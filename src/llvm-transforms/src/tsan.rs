//! ThreadSanitizer (TSan) instrumentation pass.
//!
//! This pass instruments every `Load` and `Store` instruction to call TSan
//! runtime functions, enabling detection of data races at runtime.
//!
//! # What it does
//!
//! For each non-declaration function, the pass:
//!
//! 1. **Declares** the TSan runtime functions (e.g. `__tsan_read4`,
//!    `__tsan_write4`) in the module if not already declared.
//! 2. **Adds a module constructor** `@__tsan_module_ctor` that calls `@__tsan_init`.
//! 3. **Instruments every Load** by inserting `__tsan_read{N}(ptr)` before it,
//!    where N is the byte-width of the loaded type (1, 2, 4, or 8).
//!    Loads from `alloca`-derived pointers are skipped (thread-local).
//! 4. **Instruments every Store** by inserting `__tsan_write{N}(ptr)` before it.
//! 5. **At function entry**: inserts `__tsan_func_entry(null)` as the first
//!    instruction.
//! 6. **Before each Ret**: inserts `__tsan_func_exit()`.

use crate::pass::ModulePass;
use llvm_ir::{
    BasicBlock, Context, FunctionId, InstrId, InstrKind, Instruction, Linkage, Module,
    TailCallKind, ValueRef,
};

/// ThreadSanitizer instrumentation pass.
///
/// Inserts TSan read/write barriers before every `load` and `store`,
/// emits function entry/exit hooks, and adds a `@__tsan_module_ctor`
/// that calls `@__tsan_init` at program startup.
#[derive(Debug, Clone, Default)]
pub struct TsanPass;

impl ModulePass for TsanPass {
    fn name(&self) -> &'static str {
        "tsan"
    }

    fn run_on_module(&mut self, ctx: &mut Context, module: &mut Module) -> bool {
        // Step 1: declare all TSan runtime functions.
        ensure_declarations(ctx, module);

        // Step 2: add module constructor that calls __tsan_init.
        add_module_ctor(ctx, module);

        // Step 3: instrument each non-declaration function.
        let num_funcs = module.functions.len();
        let mut changed = false;
        for fi in 0..num_funcs {
            if module.functions[fi].is_declaration {
                continue;
            }
            // Skip our own injected ctor.
            if module.functions[fi].name == "__tsan_module_ctor" {
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

/// TSan runtime function signatures: (name, num_ptr_args).
/// All return void. `__tsan_func_entry` takes one i8* arg; the rest take one i8*.
static TSAN_RUNTIME_FNS: &[(&str, usize)] = &[
    ("__tsan_init", 0),
    ("__tsan_func_entry", 1),
    ("__tsan_func_exit", 0),
    ("__tsan_read1", 1),
    ("__tsan_read2", 1),
    ("__tsan_read4", 1),
    ("__tsan_read8", 1),
    ("__tsan_write1", 1),
    ("__tsan_write2", 1),
    ("__tsan_write4", 1),
    ("__tsan_write8", 1),
];

fn ensure_declarations(ctx: &mut Context, module: &mut Module) {
    for &(name, nargs) in TSAN_RUNTIME_FNS {
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
    const CTOR_NAME: &str = "__tsan_module_ctor";
    if module.get_function_id(CTOR_NAME).is_some() {
        return;
    }

    let void_ty = ctx.void_ty;
    let fn_ty = ctx.mk_fn_type(void_ty, vec![], false);
    let mut ctor = llvm_ir::Function::new(CTOR_NAME, fn_ty, vec![], Linkage::External);

    let mut entry_bb = BasicBlock::new("entry");

    let init_fid = module
        .get_function_id("__tsan_init")
        .expect("__tsan_init must be declared first");
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

    // Collect alloca-derived pointers in the entry block for skipping.
    let alloca_vrefs = collect_alloca_vrefs(module, fid);

    // Insert func_entry at the very beginning of the entry block.
    changed |= insert_func_entry(ctx, module, fid);

    // Insert func_exit before each Ret.
    changed |= insert_func_exits(ctx, module, fid);

    // Instrument loads and stores.
    changed |= instrument_memory_accesses(ctx, module, fid, &alloca_vrefs);

    changed
}

/// Collect the ValueRefs produced by alloca instructions in the entry block.
/// These are thread-local and do not need TSan instrumentation.
fn collect_alloca_vrefs(module: &Module, fid: FunctionId) -> std::collections::HashSet<InstrId> {
    let func = &module.functions[fid.0 as usize];
    let mut set = std::collections::HashSet::new();
    if func.blocks.is_empty() {
        return set;
    }
    for &iid in &func.blocks[0].body {
        if matches!(func.instr(iid).kind, InstrKind::Alloca { .. }) {
            set.insert(iid);
        }
    }
    set
}

/// Insert `__tsan_func_entry(null_i64)` at the very start of the entry block.
fn insert_func_entry(ctx: &mut Context, module: &mut Module, fid: FunctionId) -> bool {
    let entry_fid = match module.get_function_id("__tsan_func_entry") {
        Some(id) => id,
        None => return false,
    };

    let void_ty = ctx.void_ty;
    let i64_ty = ctx.i64_ty;
    let null_arg = ctx.const_int(i64_ty, 0);
    let null_ref = ValueRef::Constant(null_arg);

    let entry_fn_ty = module.functions[entry_fid.0 as usize].ty;
    let entry_callee = ValueRef::Global(llvm_ir::GlobalId(entry_fid.0));

    let func = &mut module.functions[fid.0 as usize];
    if func.blocks.is_empty() {
        return false;
    }

    let call_iid = func.alloc_instr(Instruction {
        name: None,
        ty: void_ty,
        kind: InstrKind::Call {
            tail: TailCallKind::None,
            callee_ty: entry_fn_ty,
            callee: entry_callee,
            args: vec![null_ref],
        },
    });

    // Insert at the very beginning of the entry block (before any existing instructions).
    func.blocks[0].body.insert(0, call_iid);
    true
}

/// Insert `__tsan_func_exit()` before every `Ret` instruction.
fn insert_func_exits(ctx: &mut Context, module: &mut Module, fid: FunctionId) -> bool {
    let exit_fid = match module.get_function_id("__tsan_func_exit") {
        Some(id) => id,
        None => return false,
    };

    let void_ty = ctx.void_ty;
    let exit_fn_ty = module.functions[exit_fid.0 as usize].ty;
    let exit_callee = ValueRef::Global(llvm_ir::GlobalId(exit_fid.0));

    // Collect (block_idx) where terminator is Ret.
    let func = &module.functions[fid.0 as usize];
    let ret_blocks: Vec<usize> = func
        .blocks
        .iter()
        .enumerate()
        .filter_map(|(bi, bb)| {
            bb.terminator.and_then(|term_iid| {
                if matches!(func.instr(term_iid).kind, InstrKind::Ret { .. }) {
                    Some(bi)
                } else {
                    None
                }
            })
        })
        .collect();

    if ret_blocks.is_empty() {
        return false;
    }

    for bi in ret_blocks {
        let func = &mut module.functions[fid.0 as usize];
        let call_iid = func.alloc_instr(Instruction {
            name: None,
            ty: void_ty,
            kind: InstrKind::Call {
                tail: TailCallKind::None,
                callee_ty: exit_fn_ty,
                callee: exit_callee,
                args: vec![],
            },
        });
        // Insert just before the terminator (end of body).
        let body_len = func.blocks[bi].body.len();
        func.blocks[bi].body.insert(body_len, call_iid);
    }

    true
}

// ---------------------------------------------------------------------------
// Load/Store instrumentation
// ---------------------------------------------------------------------------

fn instrument_memory_accesses(
    ctx: &mut Context,
    module: &mut Module,
    fid: FunctionId,
    alloca_vrefs: &std::collections::HashSet<InstrId>,
) -> bool {
    struct MemAccess {
        block_idx: usize,
        body_pos: usize,
        is_load: bool,
        access_bytes: u64,
        ptr: ValueRef,
        skip: bool, // alloca-derived → skip
    }

    let func = &module.functions[fid.0 as usize];
    let mut accesses: Vec<MemAccess> = Vec::new();

    for (bi, bb) in func.blocks.iter().enumerate() {
        for (pos, &iid) in bb.body.iter().enumerate() {
            match &func.instr(iid).kind {
                InstrKind::Load { ty, ptr, .. } => {
                    let bytes = type_size_bytes(ctx, *ty).unwrap_or(4);
                    let skip = is_alloca_derived(func, *ptr, alloca_vrefs);
                    accesses.push(MemAccess {
                        block_idx: bi,
                        body_pos: pos,
                        is_load: true,
                        access_bytes: bytes,
                        ptr: *ptr,
                        skip,
                    });
                }
                InstrKind::Store { val, ptr, .. } => {
                    let val_ty = value_type(func, *val);
                    let bytes = val_ty
                        .and_then(|ty| type_size_bytes(ctx, ty))
                        .unwrap_or(4);
                    let skip = is_alloca_derived(func, *ptr, alloca_vrefs);
                    accesses.push(MemAccess {
                        block_idx: bi,
                        body_pos: pos,
                        is_load: false,
                        access_bytes: bytes,
                        ptr: *ptr,
                        skip,
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

    // We need to insert a call before each access. Process in reverse order
    // (by block then by pos) so that earlier insertion points remain valid.
    accesses.sort_by(|a, b| {
        b.block_idx
            .cmp(&a.block_idx)
            .then(b.body_pos.cmp(&a.body_pos))
    });

    let mut any = false;
    for acc in accesses {
        if acc.skip {
            continue;
        }

        let rt_name = if acc.is_load {
            match acc.access_bytes {
                1 => "__tsan_read1",
                2 => "__tsan_read2",
                8 => "__tsan_read8",
                _ => "__tsan_read4",
            }
        } else {
            match acc.access_bytes {
                1 => "__tsan_write1",
                2 => "__tsan_write2",
                8 => "__tsan_write8",
                _ => "__tsan_write4",
            }
        };

        let rt_fid = match module.get_function_id(rt_name) {
            Some(id) => id,
            None => continue,
        };
        let rt_fn_ty = module.functions[rt_fid.0 as usize].ty;
        let rt_callee = ValueRef::Global(llvm_ir::GlobalId(rt_fid.0));

        // Cast the pointer to i64 for the TSan runtime call.
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

        // Insert both instructions before body_pos.
        func.blocks[acc.block_idx]
            .body
            .insert(acc.body_pos, call_iid);
        func.blocks[acc.block_idx]
            .body
            .insert(acc.body_pos, ptr_int_iid);
        any = true;
    }

    any
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Return the byte-size of a type for selecting the TSan function.
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

/// Return the type of an SSA value within the function.
fn value_type(func: &llvm_ir::Function, vref: ValueRef) -> Option<llvm_ir::TypeId> {
    match vref {
        ValueRef::Instruction(iid) => Some(func.instr(iid).ty),
        ValueRef::Argument(aid) => Some(func.arg(aid).ty),
        _ => None,
    }
}

/// Return true if `ptr` is directly an alloca (thread-local stack slot).
fn is_alloca_derived(
    _func: &llvm_ir::Function,
    ptr: ValueRef,
    alloca_set: &std::collections::HashSet<InstrId>,
) -> bool {
    match ptr {
        ValueRef::Instruction(iid) => alloca_set.contains(&iid),
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pass::ModulePass;
    use llvm_ir::{Builder, Context, Function, InstrKind, Linkage, Module};

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

    fn make_store_module_i64() -> (Context, Module) {
        let mut ctx = Context::new();
        let mut module = Module::new("test");
        let mut b = Builder::new(&mut ctx, &mut module);
        let ptr_ty = b.ctx.ptr_ty;
        let i64_ty = b.ctx.i64_ty;
        b.add_function(
            "f",
            b.ctx.void_ty,
            vec![i64_ty, ptr_ty],
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

    fn make_alloca_load_module() -> (Context, Module) {
        let mut ctx = Context::new();
        let mut module = Module::new("test");
        let mut b = Builder::new(&mut ctx, &mut module);
        let i32_ty = b.ctx.i32_ty;
        b.add_function("f", b.ctx.void_ty, vec![], vec![], false, Linkage::External);
        let entry = b.add_block("entry");
        b.position_at_end(entry);
        let slot = b.build_alloca("slot", i32_ty);
        // load from the alloca — should NOT be instrumented
        let _v = b.build_load("v", i32_ty, slot);
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

    fn make_load_module_i32() -> (Context, Module) {
        let mut ctx = Context::new();
        let mut module = Module::new("test");
        let mut b = Builder::new(&mut ctx, &mut module);
        let ptr_ty = b.ctx.ptr_ty;
        let i32_ty = b.ctx.i32_ty;
        b.add_function(
            "f",
            i32_ty,
            vec![ptr_ty],
            vec!["p".into()],
            false,
            Linkage::External,
        );
        let entry = b.add_block("entry");
        b.position_at_end(entry);
        let p = b.get_arg(0);
        let v = b.build_load("v", i32_ty, p);
        b.build_ret(v);
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
    fn tsan_inserts_read_before_load() {
        let (mut ctx, mut module) = make_load_module_i64();
        let mut pass = TsanPass;
        pass.run_on_module(&mut ctx, &mut module);
        // f is function index 0
        let count = count_calls_to(&module, 0, "__tsan_read8");
        assert!(count >= 1, "__tsan_read8 should be called at least once, got {count}");
    }

    #[test]
    fn tsan_inserts_write_before_store() {
        let (mut ctx, mut module) = make_store_module_i64();
        let mut pass = TsanPass;
        pass.run_on_module(&mut ctx, &mut module);
        let count = count_calls_to(&module, 0, "__tsan_write8");
        assert!(count >= 1, "__tsan_write8 should be called at least once, got {count}");
    }

    #[test]
    fn tsan_skips_alloca_pointer() {
        let (mut ctx, mut module) = make_alloca_load_module();
        let before_instr_count = module.functions[0].instructions.len();
        let mut pass = TsanPass;
        pass.run_on_module(&mut ctx, &mut module);
        // The load from alloca should not generate tsan_read calls.
        let read_count = count_calls_to(&module, 0, "__tsan_read4");
        let read1_count = count_calls_to(&module, 0, "__tsan_read1");
        let read8_count = count_calls_to(&module, 0, "__tsan_read8");
        assert_eq!(
            read_count + read1_count + read8_count,
            0,
            "alloca-derived load must not be instrumented"
        );
        // But func_entry and func_exit should still be added.
        let after_instr_count = module.functions[0].instructions.len();
        assert!(
            after_instr_count > before_instr_count,
            "func_entry/exit hooks should still be inserted"
        );
    }

    #[test]
    fn tsan_inserts_func_entry_exit() {
        let (mut ctx, mut module) = make_load_module_i64();
        let mut pass = TsanPass;
        pass.run_on_module(&mut ctx, &mut module);
        let entry_count = count_calls_to(&module, 0, "__tsan_func_entry");
        let exit_count = count_calls_to(&module, 0, "__tsan_func_exit");
        assert!(entry_count >= 1, "__tsan_func_entry must be inserted");
        assert!(exit_count >= 1, "__tsan_func_exit must be inserted before ret");
    }

    #[test]
    fn tsan_declares_runtime_fns() {
        let (mut ctx, mut module) = make_load_module_i64();
        let mut pass = TsanPass;
        pass.run_on_module(&mut ctx, &mut module);
        for name in &[
            "__tsan_init",
            "__tsan_func_entry",
            "__tsan_func_exit",
            "__tsan_read1",
            "__tsan_read2",
            "__tsan_read4",
            "__tsan_read8",
            "__tsan_write1",
            "__tsan_write2",
            "__tsan_write4",
            "__tsan_write8",
        ] {
            assert!(
                module.get_function_id(name).is_some(),
                "{name} must be declared"
            );
        }
    }

    #[test]
    fn tsan_byte_width_i8() {
        let (mut ctx, mut module) = make_load_module_i8();
        let mut pass = TsanPass;
        pass.run_on_module(&mut ctx, &mut module);
        let count = count_calls_to(&module, 0, "__tsan_read1");
        assert!(count >= 1, "i8 load should use __tsan_read1, got {count}");
    }

    #[test]
    fn tsan_byte_width_i32() {
        let (mut ctx, mut module) = make_load_module_i32();
        let mut pass = TsanPass;
        pass.run_on_module(&mut ctx, &mut module);
        let count = count_calls_to(&module, 0, "__tsan_read4");
        assert!(count >= 1, "i32 load should use __tsan_read4, got {count}");
    }

    #[test]
    fn tsan_module_ctor_present() {
        let (mut ctx, mut module) = make_load_module_i64();
        let mut pass = TsanPass;
        pass.run_on_module(&mut ctx, &mut module);
        assert!(
            module.get_function_id("__tsan_module_ctor").is_some(),
            "__tsan_module_ctor must be added"
        );
        // Verify it calls __tsan_init.
        let ctor_fid = module.get_function_id("__tsan_module_ctor").unwrap();
        let init_count = count_calls_to(&module, ctor_fid.0 as usize, "__tsan_init");
        assert!(init_count >= 1, "__tsan_module_ctor must call __tsan_init");
    }
}
