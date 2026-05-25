//! MemorySanitizer (MSan) instrumentation pass.
//!
//! This pass instruments memory operations to detect use of uninitialized
//! memory at runtime, using a call-based (soft-mode) approach.
//!
//! # What it does
//!
//! For each non-declaration function, the pass:
//!
//! 1. **Declares** the MSan runtime functions in the module if not already
//!    present.
//! 2. **Adds a module constructor** `@__msan_module_ctor` that calls
//!    `@__msan_init`.
//! 3. **After each Alloca**: inserts `__msan_poison_stack(ptr, size)` — stack
//!    memory starts logically uninitialized.
//! 4. **Before each Store of N bytes**: inserts `__msan_store_shadow{N}(ptr)`
//!    to mark those bytes as initialized in the shadow.
//! 5. **After each Load of N bytes**: inserts `__msan_check_shadow{N}(ptr)` to
//!    verify the bytes are initialized; the runtime calls
//!    `__msan_warning_noreturn()` on a violation.

use crate::pass::ModulePass;
use llvm_ir::{
    BasicBlock, Context, FunctionId, InstrId, InstrKind, Instruction, Linkage, Module,
    TailCallKind, ValueRef,
};

/// MemorySanitizer instrumentation pass.
///
/// Inserts shadow-memory checks after every `load`, shadow-store calls
/// before every `store`, and poisons `alloca` stack slots.
/// Adds a `@__msan_module_ctor` that calls `@__msan_init` at startup.
#[derive(Debug, Clone, Default)]
pub struct MsanPass;

impl ModulePass for MsanPass {
    fn name(&self) -> &'static str {
        "msan"
    }

    fn run_on_module(&mut self, ctx: &mut Context, module: &mut Module) -> bool {
        ensure_declarations(ctx, module);
        add_module_ctor(ctx, module);

        let num_funcs = module.functions.len();
        let mut changed = false;
        for fi in 0..num_funcs {
            if module.functions[fi].is_declaration {
                continue;
            }
            if module.functions[fi].name == "__msan_module_ctor" {
                continue;
            }
            changed |= instrument_function(ctx, module, FunctionId(fi as u32));
        }
        changed
    }
}

// ---------------------------------------------------------------------------
// Runtime declarations
// ---------------------------------------------------------------------------

/// MSan runtime functions: (name, param_count).
/// All return void.  Params are either zero, one i8*, or (i8*, i64).
static MSAN_RUNTIME_FNS: &[(&str, usize)] = &[
    ("__msan_init", 0),
    ("__msan_warning_noreturn", 0),
    // poison_stack takes (ptr: i8*, size: i64) — two i64 params (both cast to i64)
    ("__msan_poison_stack", 2),
    ("__msan_check_shadow1", 1),
    ("__msan_check_shadow2", 1),
    ("__msan_check_shadow4", 1),
    ("__msan_check_shadow8", 1),
    ("__msan_store_shadow1", 1),
    ("__msan_store_shadow2", 1),
    ("__msan_store_shadow4", 1),
    ("__msan_store_shadow8", 1),
];

fn ensure_declarations(ctx: &mut Context, module: &mut Module) {
    for &(name, nargs) in MSAN_RUNTIME_FNS {
        if module.get_function_id(name).is_some() {
            continue;
        }
        let void_ty = ctx.void_ty;
        let i64_ty = ctx.i64_ty;
        let params: Vec<_> = (0..nargs).map(|_| i64_ty).collect();
        let fn_ty = ctx.mk_fn_type(void_ty, params.clone(), false);
        use llvm_ir::{Argument, Function};
        let args: Vec<Argument> = params
            .iter()
            .enumerate()
            .map(|(i, &ty)| Argument {
                name: String::new(),
                ty,
                index: i as u32,
            })
            .collect();
        let decl = Function::new_declaration(name, fn_ty, args, Linkage::External);
        module.add_function(decl);
    }
}

// ---------------------------------------------------------------------------
// Module constructor
// ---------------------------------------------------------------------------

fn add_module_ctor(ctx: &mut Context, module: &mut Module) {
    const CTOR_NAME: &str = "__msan_module_ctor";
    if module.get_function_id(CTOR_NAME).is_some() {
        return;
    }

    let void_ty = ctx.void_ty;
    let fn_ty = ctx.mk_fn_type(void_ty, vec![], false);
    let mut ctor = llvm_ir::Function::new(CTOR_NAME, fn_ty, vec![], Linkage::External);
    let mut entry_bb = BasicBlock::new("entry");

    let init_fid = module
        .get_function_id("__msan_init")
        .expect("__msan_init must be declared first");
    let init_fn_ty = module.functions[init_fid.0 as usize].ty;

    let call_iid = ctor.alloc_instr(Instruction {
        name: None,
        ty: void_ty,
        kind: InstrKind::Call {
            tail: TailCallKind::None,
            callee_ty: init_fn_ty,
            callee: ValueRef::Global(llvm_ir::GlobalId(init_fid.0)),
            args: vec![],
        },
    });
    entry_bb.body.push(call_iid);

    let ret_iid = ctor.alloc_instr(Instruction {
        name: None,
        ty: void_ty,
        kind: InstrKind::Ret { val: None },
    });
    entry_bb.set_terminator(ret_iid);
    ctor.add_block(entry_bb);
    module.add_function(ctor);
}

// ---------------------------------------------------------------------------
// Per-function instrumentation
// ---------------------------------------------------------------------------

fn instrument_function(ctx: &mut Context, module: &mut Module, fid: FunctionId) -> bool {
    let mut changed = false;
    changed |= instrument_allocas(ctx, module, fid);
    changed |= instrument_memory_accesses(ctx, module, fid);
    changed
}

// ---------------------------------------------------------------------------
// Alloca poisoning
// ---------------------------------------------------------------------------

/// After each alloca insert `__msan_poison_stack(ptr_as_i64, size_as_i64)`.
fn instrument_allocas(ctx: &mut Context, module: &mut Module, fid: FunctionId) -> bool {
    let poison_fid = match module.get_function_id("__msan_poison_stack") {
        Some(id) => id,
        None => return false,
    };

    let func = &module.functions[fid.0 as usize];
    // Collect (block_idx, body_pos, iid, size_bytes) for all allocas.
    let mut allocas: Vec<(usize, usize, InstrId, u64)> = Vec::new();
    for (bi, bb) in func.blocks.iter().enumerate() {
        for (pos, &iid) in bb.body.iter().enumerate() {
            if let InstrKind::Alloca { alloc_ty, .. } = &func.instr(iid).kind {
                let size = type_size_bytes(ctx, *alloc_ty).unwrap_or(4);
                allocas.push((bi, pos, iid, size));
            }
        }
    }

    if allocas.is_empty() {
        return false;
    }

    let void_ty = ctx.void_ty;
    let i64_ty = ctx.i64_ty;
    let poison_fn_ty = module.functions[poison_fid.0 as usize].ty;
    let poison_callee = ValueRef::Global(llvm_ir::GlobalId(poison_fid.0));

    // Process in reverse order (block desc, pos desc) so indices stay valid.
    allocas.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.cmp(&a.1)));

    for (bi, pos, alloca_iid, size) in allocas {
        let size_const = ctx.const_int(i64_ty, size);
        let size_ref = ValueRef::Constant(size_const);

        let func = &mut module.functions[fid.0 as usize];

        let ptr_int_name = func.fresh_name();
        let ptr_int_iid = func.alloc_instr(Instruction {
            name: Some(ptr_int_name),
            ty: i64_ty,
            kind: InstrKind::PtrToInt {
                val: ValueRef::Instruction(alloca_iid),
                to: i64_ty,
            },
        });

        let call_iid = func.alloc_instr(Instruction {
            name: None,
            ty: void_ty,
            kind: InstrKind::Call {
                tail: TailCallKind::None,
                callee_ty: poison_fn_ty,
                callee: poison_callee,
                args: vec![ValueRef::Instruction(ptr_int_iid), size_ref],
            },
        });

        // Insert immediately after the alloca: first ptr_int, then call.
        // After inserting ptr_int at pos+1, call goes at pos+2.
        let after_alloca = pos + 1;
        func.blocks[bi].body.insert(after_alloca, ptr_int_iid);
        func.blocks[bi].body.insert(after_alloca + 1, call_iid);
    }

    true
}

// ---------------------------------------------------------------------------
// Load/Store shadow instrumentation
// ---------------------------------------------------------------------------

fn instrument_memory_accesses(ctx: &mut Context, module: &mut Module, fid: FunctionId) -> bool {
    struct MemAccess {
        block_idx: usize,
        body_pos: usize,
        is_load: bool,
        access_bytes: u64,
        ptr: ValueRef,
    }

    let func = &module.functions[fid.0 as usize];
    let mut accesses: Vec<MemAccess> = Vec::new();

    for (bi, bb) in func.blocks.iter().enumerate() {
        for (pos, &iid) in bb.body.iter().enumerate() {
            match &func.instr(iid).kind {
                InstrKind::Load { ty, ptr, .. } => {
                    let bytes = type_size_bytes(ctx, *ty).unwrap_or(4);
                    accesses.push(MemAccess {
                        block_idx: bi,
                        body_pos: pos,
                        is_load: true,
                        access_bytes: bytes,
                        ptr: *ptr,
                    });
                }
                InstrKind::Store { val, ptr, .. } => {
                    let val_ty = value_type(func, *val);
                    let bytes = val_ty
                        .and_then(|ty| type_size_bytes(ctx, ty))
                        .unwrap_or(4);
                    accesses.push(MemAccess {
                        block_idx: bi,
                        body_pos: pos,
                        is_load: false,
                        access_bytes: bytes,
                        ptr: *ptr,
                    });
                }
                _ => {}
            }
        }
    }

    if accesses.is_empty() {
        return false;
    }

    let void_ty = ctx.void_ty;
    let i64_ty = ctx.i64_ty;

    // Process in reverse block/pos order so insertions don't shift pending indices.
    accesses.sort_by(|a, b| {
        b.block_idx
            .cmp(&a.block_idx)
            .then(b.body_pos.cmp(&a.body_pos))
    });

    let mut any = false;
    for acc in accesses {
        let rt_name = if acc.is_load {
            // After load: check shadow
            match acc.access_bytes {
                1 => "__msan_check_shadow1",
                2 => "__msan_check_shadow2",
                8 => "__msan_check_shadow8",
                _ => "__msan_check_shadow4",
            }
        } else {
            // Before store: mark shadow
            match acc.access_bytes {
                1 => "__msan_store_shadow1",
                2 => "__msan_store_shadow2",
                8 => "__msan_store_shadow8",
                _ => "__msan_store_shadow4",
            }
        };

        let rt_fid = match module.get_function_id(rt_name) {
            Some(id) => id,
            None => continue,
        };
        let rt_fn_ty = module.functions[rt_fid.0 as usize].ty;
        let rt_callee = ValueRef::Global(llvm_ir::GlobalId(rt_fid.0));

        let func = &mut module.functions[fid.0 as usize];

        let ptr_int_name = func.fresh_name();
        let ptr_int_iid = func.alloc_instr(Instruction {
            name: Some(ptr_int_name),
            ty: i64_ty,
            kind: InstrKind::PtrToInt {
                val: acc.ptr,
                to: i64_ty,
            },
        });

        let call_iid = func.alloc_instr(Instruction {
            name: None,
            ty: void_ty,
            kind: InstrKind::Call {
                tail: TailCallKind::None,
                callee_ty: rt_fn_ty,
                callee: rt_callee,
                args: vec![ValueRef::Instruction(ptr_int_iid)],
            },
        });

        if acc.is_load {
            // Insert AFTER the load: ptr_int at load_pos+1, call at load_pos+2.
            let after_load = acc.body_pos + 1;
            func.blocks[acc.block_idx]
                .body
                .insert(after_load, ptr_int_iid);
            func.blocks[acc.block_idx]
                .body
                .insert(after_load + 1, call_iid);
        } else {
            // Insert BEFORE the store (mark shadow as initialized).
            func.blocks[acc.block_idx]
                .body
                .insert(acc.body_pos, call_iid);
            func.blocks[acc.block_idx]
                .body
                .insert(acc.body_pos, ptr_int_iid);
        }

        any = true;
    }

    any
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn type_size_bytes(ctx: &Context, ty: llvm_ir::TypeId) -> Option<u64> {
    use llvm_ir::TypeData;
    match ctx.get_type(ty) {
        TypeData::Integer(bits) => Some((*bits as u64).div_ceil(8)),
        TypeData::Float(llvm_ir::FloatKind::Single) => Some(4),
        TypeData::Float(llvm_ir::FloatKind::Double) => Some(8),
        TypeData::Pointer => Some(8),
        _ => None,
    }
}

fn value_type(func: &llvm_ir::Function, vref: ValueRef) -> Option<llvm_ir::TypeId> {
    match vref {
        ValueRef::Instruction(iid) => Some(func.instr(iid).ty),
        ValueRef::Argument(aid) => Some(func.arg(aid).ty),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pass::ModulePass;
    use llvm_ir::{Builder, Context, InstrKind, Linkage, Module};

    fn make_load_module_i64() -> (Context, Module) {
        let mut ctx = Context::new();
        let mut module = Module::new("test");
        let mut b = Builder::new(&mut ctx, &mut module);
        let ptr_ty = b.ctx.ptr_ty;
        let i64_ty = b.ctx.i64_ty;
        b.add_function(
            "f",
            i64_ty,
            vec![ptr_ty],
            vec!["p".into()],
            false,
            Linkage::External,
        );
        let entry = b.add_block("entry");
        b.position_at_end(entry);
        let p = b.get_arg(0);
        let v = b.build_load("v", i64_ty, p);
        b.build_ret(v);
        (ctx, module)
    }

    fn make_store_module_i32() -> (Context, Module) {
        let mut ctx = Context::new();
        let mut module = Module::new("test");
        let mut b = Builder::new(&mut ctx, &mut module);
        let ptr_ty = b.ctx.ptr_ty;
        let i32_ty = b.ctx.i32_ty;
        b.add_function(
            "f",
            b.ctx.void_ty,
            vec![i32_ty, ptr_ty],
            vec!["val".into(), "p".into()],
            false,
            Linkage::External,
        );
        let entry = b.add_block("entry");
        b.position_at_end(entry);
        let val = b.get_arg(0);
        let p = b.get_arg(1);
        b.build_store(val, p);
        b.build_ret_void();
        (ctx, module)
    }

    fn make_alloca_module() -> (Context, Module) {
        let mut ctx = Context::new();
        let mut module = Module::new("test");
        let mut b = Builder::new(&mut ctx, &mut module);
        let i32_ty = b.ctx.i32_ty;
        b.add_function("f", b.ctx.void_ty, vec![], vec![], false, Linkage::External);
        let entry = b.add_block("entry");
        b.position_at_end(entry);
        let _slot = b.build_alloca("slot", i32_ty);
        b.build_ret_void();
        (ctx, module)
    }

    fn make_load_module_i8() -> (Context, Module) {
        let mut ctx = Context::new();
        let mut module = Module::new("test");
        let mut b = Builder::new(&mut ctx, &mut module);
        let ptr_ty = b.ctx.ptr_ty;
        let i8_ty = b.ctx.i8_ty;
        b.add_function(
            "f",
            i8_ty,
            vec![ptr_ty],
            vec!["p".into()],
            false,
            Linkage::External,
        );
        let entry = b.add_block("entry");
        b.position_at_end(entry);
        let p = b.get_arg(0);
        let v = b.build_load("v", i8_ty, p);
        b.build_ret(v);
        (ctx, module)
    }

    fn make_store_load_module() -> (Context, Module) {
        // f(ptr %p, i32 %val): store val to p, then load i32 from p
        let mut ctx = Context::new();
        let mut module = Module::new("test");
        let mut b = Builder::new(&mut ctx, &mut module);
        let ptr_ty = b.ctx.ptr_ty;
        let i32_ty = b.ctx.i32_ty;
        b.add_function(
            "f",
            i32_ty,
            vec![ptr_ty, i32_ty],
            vec!["p".into(), "val".into()],
            false,
            Linkage::External,
        );
        let entry = b.add_block("entry");
        b.position_at_end(entry);
        let p = b.get_arg(0);
        let val = b.get_arg(1);
        b.build_store(val, p);
        let loaded = b.build_load("loaded", i32_ty, p);
        b.build_ret(loaded);
        (ctx, module)
    }

    fn count_calls_to(module: &Module, fi: usize, name: &str) -> usize {
        let target_fid = match module.get_function_id(name) {
            Some(id) => id,
            None => return 0,
        };
        let target_global = llvm_ir::GlobalId(target_fid.0);
        module.functions[fi]
            .blocks
            .iter()
            .flat_map(|bb| bb.body.iter())
            .filter(|&&iid| {
                matches!(
                    &module.functions[fi].instr(iid).kind,
                    InstrKind::Call { callee: ValueRef::Global(gid), .. } if *gid == target_global
                )
            })
            .count()
    }

    #[test]
    fn msan_load_gets_check() {
        let (mut ctx, mut module) = make_load_module_i64();
        let mut pass = MsanPass;
        pass.run_on_module(&mut ctx, &mut module);
        let count = count_calls_to(&module, 0, "__msan_check_shadow8");
        assert!(count >= 1, "__msan_check_shadow8 should be inserted for i64 load, got {count}");
    }

    #[test]
    fn msan_store_gets_mark() {
        let (mut ctx, mut module) = make_store_module_i32();
        let mut pass = MsanPass;
        pass.run_on_module(&mut ctx, &mut module);
        let count = count_calls_to(&module, 0, "__msan_store_shadow4");
        assert!(count >= 1, "__msan_store_shadow4 should be inserted for i32 store, got {count}");
    }

    #[test]
    fn msan_alloca_gets_poison() {
        let (mut ctx, mut module) = make_alloca_module();
        let mut pass = MsanPass;
        pass.run_on_module(&mut ctx, &mut module);
        let count = count_calls_to(&module, 0, "__msan_poison_stack");
        assert!(count >= 1, "__msan_poison_stack should be inserted after alloca, got {count}");
    }

    #[test]
    fn msan_declares_runtime_fns() {
        let (mut ctx, mut module) = make_load_module_i64();
        let mut pass = MsanPass;
        pass.run_on_module(&mut ctx, &mut module);
        for name in &[
            "__msan_init",
            "__msan_warning_noreturn",
            "__msan_poison_stack",
            "__msan_check_shadow1",
            "__msan_check_shadow2",
            "__msan_check_shadow4",
            "__msan_check_shadow8",
            "__msan_store_shadow1",
            "__msan_store_shadow2",
            "__msan_store_shadow4",
            "__msan_store_shadow8",
        ] {
            assert!(
                module.get_function_id(name).is_some(),
                "{name} must be declared"
            );
        }
    }

    #[test]
    fn msan_module_ctor_present() {
        let (mut ctx, mut module) = make_load_module_i64();
        let mut pass = MsanPass;
        pass.run_on_module(&mut ctx, &mut module);
        let ctor_fid = module
            .get_function_id("__msan_module_ctor")
            .expect("__msan_module_ctor must be added");
        let init_count = count_calls_to(&module, ctor_fid.0 as usize, "__msan_init");
        assert!(init_count >= 1, "__msan_module_ctor must call __msan_init");
    }

    #[test]
    fn msan_byte_width_i8_load() {
        let (mut ctx, mut module) = make_load_module_i8();
        let mut pass = MsanPass;
        pass.run_on_module(&mut ctx, &mut module);
        let count = count_calls_to(&module, 0, "__msan_check_shadow1");
        assert!(count >= 1, "i8 load should use __msan_check_shadow1, got {count}");
    }

    #[test]
    fn msan_byte_width_i32_store() {
        let (mut ctx, mut module) = make_store_module_i32();
        let mut pass = MsanPass;
        pass.run_on_module(&mut ctx, &mut module);
        let count = count_calls_to(&module, 0, "__msan_store_shadow4");
        assert!(count >= 1, "i32 store should use __msan_store_shadow4, got {count}");
    }

    #[test]
    fn msan_load_after_store_no_double_check() {
        // Both the store (shadow4) AND the load (check4) should be independently
        // instrumented. MSan instruments all accesses; the runtime tracks state.
        let (mut ctx, mut module) = make_store_load_module();
        let mut pass = MsanPass;
        pass.run_on_module(&mut ctx, &mut module);
        let store_count = count_calls_to(&module, 0, "__msan_store_shadow4");
        let load_count = count_calls_to(&module, 0, "__msan_check_shadow4");
        assert!(store_count >= 1, "store should get shadow mark, got {store_count}");
        assert!(load_count >= 1, "load should get shadow check, got {load_count}");
    }
}
