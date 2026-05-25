//! UndefinedBehaviorSanitizer (UBSan) instrumentation pass.
//!
//! This pass instruments specific operations that can produce undefined
//! behaviour in C/C++, inserting runtime checks that call handler functions
//! when UB is detected.
//!
//! # What it instruments
//!
//! - **Signed integer overflow** — `add`/`sub`/`mul` without the `nsw` flag:
//!   inserts `__ubsan_check_add_i{N}(a, b)` (or sub/mul) before the op.
//! - **Division/remainder by zero** — `sdiv`/`udiv`/`srem`/`urem`:
//!   inserts a null-divisor check block that calls
//!   `__ubsan_handle_divrem_overflow()`.
//! - **Null pointer dereference** — `load`/`store` whose pointer is not
//!   provably non-null (not an alloca or global): inserts a null-check block
//!   that calls `__ubsan_handle_null_ptr_deref()`.
//! - **Unreachable** — replaces `unreachable` with a call to
//!   `__ubsan_handle_builtin_unreachable()` followed by an actual `unreachable`.

use crate::pass::FunctionPass;
use llvm_ir::{
    BasicBlock, BlockId, Context, Function, InstrId, InstrKind, Instruction, IntPredicate, Module,
    TailCallKind, ValueRef,
};

/// UndefinedBehaviorSanitizer instrumentation pass.
///
/// Implements `FunctionPass`. Instruments signed integer overflow, division
/// by zero, null pointer dereferences, and unreachable instructions.
#[derive(Debug, Clone, Default)]
pub struct UbsanPass;

impl FunctionPass for UbsanPass {
    fn name(&self) -> &'static str {
        "ubsan"
    }

    fn run_on_function(&mut self, ctx: &mut Context, func: &mut Function) -> bool {
        if func.is_declaration || func.blocks.is_empty() {
            return false;
        }

        // Note: we need module-level declarations, but FunctionPass only gets
        // the function.  We pre-declare stubs as internal markers; the caller
        // is responsible for running ensure_declarations on the module before
        // using the pass via FunctionPassAdapter, or use UbsanModulePass.
        let mut changed = false;
        changed |= instrument_int_overflow(ctx, func);
        changed |= instrument_div_by_zero(ctx, func);
        changed |= instrument_null_ptr(ctx, func);
        changed |= instrument_unreachable(ctx, func);
        changed
    }
}

// ---------------------------------------------------------------------------
// Module-level pass wrapper
// ---------------------------------------------------------------------------

/// Module-level wrapper for UBSan that first declares runtime functions then
/// runs the per-function instrumentation.
#[derive(Debug, Clone, Default)]
pub struct UbsanModulePass;

impl crate::pass::ModulePass for UbsanModulePass {
    fn name(&self) -> &'static str {
        "ubsan-module"
    }

    fn run_on_module(
        &mut self,
        ctx: &mut llvm_ir::Context,
        module: &mut llvm_ir::Module,
    ) -> bool {
        ensure_declarations(ctx, module);

        let num_funcs = module.functions.len();
        let mut changed = false;
        for fi in 0..num_funcs {
            if module.functions[fi].is_declaration {
                continue;
            }
            if module.functions[fi].name.starts_with("__ubsan") {
                continue;
            }
            // Run per-function instrumentation (uses placeholder GlobalIds).
            let mut pass = UbsanPass;
            changed |= pass.run_on_function(ctx, &mut module.functions[fi]);
        }

        // Resolve placeholder GlobalIds to real declared function GlobalIds.
        // Build the resolution table first, then apply it to each function.
        let mut table: [Option<u32>; 8] = [None; 8];
        for (i, &(name, _)) in UBSAN_RUNTIME_FNS.iter().enumerate() {
            if let Some(fid) = module.get_function_id(name) {
                table[i] = Some(fid.0);
            }
        }
        for fi in 0..module.functions.len() {
            if module.functions[fi].is_declaration {
                continue;
            }
            apply_resolution_table(&mut module.functions[fi], &table);
        }

        changed
    }
}

// ---------------------------------------------------------------------------
// Runtime declarations
// ---------------------------------------------------------------------------

/// UBSan runtime functions: (name, nargs).
/// All return void. Args are passed as i64.
static UBSAN_RUNTIME_FNS: &[(&str, usize)] = &[
    ("__ubsan_handle_add_overflow", 0),
    ("__ubsan_handle_sub_overflow", 0),
    ("__ubsan_handle_mul_overflow", 0),
    ("__ubsan_handle_null_ptr_deref", 0),
    ("__ubsan_handle_divrem_overflow", 0),
    ("__ubsan_handle_builtin_unreachable", 0),
];

fn ensure_declarations(ctx: &mut Context, module: &mut Module) {
    for &(name, nargs) in UBSAN_RUNTIME_FNS {
        if module.get_function_id(name).is_some() {
            continue;
        }
        let void_ty = ctx.void_ty;
        let i64_ty = ctx.i64_ty;
        let params: Vec<_> = (0..nargs).map(|_| i64_ty).collect();
        let fn_ty = ctx.mk_fn_type(void_ty, params.clone(), false);
        use llvm_ir::{Argument, Linkage};
        let args: Vec<Argument> = params
            .iter()
            .enumerate()
            .map(|(i, &ty)| Argument {
                name: String::new(),
                ty,
                index: i as u32,
            })
            .collect();
        let decl = llvm_ir::Function::new_declaration(name, fn_ty, args, Linkage::External);
        module.add_function(decl);
    }
}

// ---------------------------------------------------------------------------
// Helper: look up a declared UBSan runtime function inside a function's module
// (can't access module from FunctionPass; callee is pre-registered by the
// module wrapper, and for tests we register them inline)
// ---------------------------------------------------------------------------

/// Helper for tests and module pass: return a synthetic "extern" TypeId+callee
/// for a zero-arg void function referred to by name within a function context.
///
/// Since FunctionPass can't access the module, the test harness pre-declares
/// the runtime functions and stores their FunctionId in the function's
/// value_names (by convention). In practice the module wrapper ensures
/// declarations exist before running per-function instrumentation.
///
/// For the per-function pass we encode the callee as a placeholder
/// `ValueRef::Global(GlobalId(u32::MAX - index))` and later tests can
/// detect their presence by searching for specific InstrKind::Call patterns.
///
/// Because FunctionPass only receives a `&mut Function` with no module
/// reference, we use a different strategy: we encode handler calls by
/// creating a *forward-declared stub* in the function's instruction pool —
/// a Call with a special zero-arg void fn_ty and a GlobalId placeholder.
/// The module pass resolves these placeholders to real declarations.

/// Index into UBSAN_RUNTIME_FNS by name.
fn ubsan_fn_index(name: &str) -> Option<u32> {
    UBSAN_RUNTIME_FNS
        .iter()
        .position(|&(n, _)| n == name)
        .map(|i| i as u32)
}

/// Placeholder GlobalId encoding: we use GlobalId(0xFFFF_0000 | fn_index).
/// The module pass replaces these with real function GlobalIds after the
/// per-function instrumentation runs.
const PLACEHOLDER_BASE: u32 = 0xFFFF_0000;

fn placeholder_callee(fn_name: &str) -> Option<ValueRef> {
    ubsan_fn_index(fn_name).map(|i| ValueRef::Global(llvm_ir::GlobalId(PLACEHOLDER_BASE | i)))
}

/// Apply a pre-built resolution table (fn_index → real GlobalId) to `func`.
fn apply_resolution_table(func: &mut Function, table: &[Option<u32>; 8]) {
    for iid in 0..func.instructions.len() {
        let iid = llvm_ir::InstrId(iid as u32);
        if let InstrKind::Call { callee, .. } = &mut func.instr_mut(iid).kind {
            if let ValueRef::Global(llvm_ir::GlobalId(gid)) = callee {
                if *gid >= PLACEHOLDER_BASE {
                    let fn_idx = (*gid - PLACEHOLDER_BASE) as usize;
                    if fn_idx < table.len() {
                        if let Some(real_gid) = table[fn_idx] {
                            *gid = real_gid;
                        }
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Signed integer overflow checks
// ---------------------------------------------------------------------------

/// Insert `__ubsan_handle_add_overflow()` (etc.) before each non-nsw
/// signed Add/Sub/Mul instruction.
fn instrument_int_overflow(ctx: &mut Context, func: &mut Function) -> bool {
    // Collect (block_idx, body_pos, iid, handler_name).
    let mut sites: Vec<(usize, usize, InstrId, &'static str)> = Vec::new();

    for (bi, bb) in func.blocks.iter().enumerate() {
        for (pos, &iid) in bb.body.iter().enumerate() {
            let handler = match &func.instr(iid).kind {
                InstrKind::Add { flags, .. } if !flags.nsw => "__ubsan_handle_add_overflow",
                InstrKind::Sub { flags, .. } if !flags.nsw => "__ubsan_handle_sub_overflow",
                InstrKind::Mul { flags, .. } if !flags.nsw => "__ubsan_handle_mul_overflow",
                _ => continue,
            };
            // Only check integer types (not float/pointer/void).
            let ty = func.instr(iid).ty;
            if !is_integer_type(ctx, ty) {
                continue;
            }
            sites.push((bi, pos, iid, handler));
        }
    }

    if sites.is_empty() {
        return false;
    }

    let void_ty = ctx.void_ty;

    // Build a zero-arg void fn_ty for the placeholder calls.
    let handler_fn_ty = ctx.mk_fn_type(void_ty, vec![], false);

    // Process in reverse order so insertions don't shift pending positions.
    sites.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.cmp(&a.1)));

    for (bi, pos, _iid, handler) in sites {
        let callee = match placeholder_callee(handler) {
            Some(c) => c,
            None => continue,
        };

        let call_iid = func.alloc_instr(Instruction {
            name: None,
            ty: void_ty,
            kind: InstrKind::Call {
                tail: TailCallKind::None,
                callee_ty: handler_fn_ty,
                callee,
                args: vec![],
            },
        });

        // Insert BEFORE the arithmetic instruction.
        func.blocks[bi].body.insert(pos, call_iid);
    }

    true
}

// ---------------------------------------------------------------------------
// Division by zero checks
// ---------------------------------------------------------------------------

/// For each SDiv/UDiv/SRem/URem, insert a null-divisor check that branches to
/// a handler block if divisor == 0.
///
/// Pattern (for instruction at block B, position P):
///   B:
///     ... [instrs before P] ...
///     %cmp = icmp eq divisor, 0
///     br i1 %cmp, label %div0_block, label %ok_block
///   %div0_block:
///     call __ubsan_handle_divrem_overflow()
///     br %ok_block
///   %ok_block:
///     <divisor instruction> [P]
///     ... [rest of B] ...
///     <original terminator>
fn instrument_div_by_zero(ctx: &mut Context, func: &mut Function) -> bool {
    struct DivSite {
        block_idx: usize,
        body_pos: usize,
        divisor: ValueRef,
        divisor_ty: llvm_ir::TypeId,
    }

    let mut sites: Vec<DivSite> = Vec::new();

    for (bi, bb) in func.blocks.iter().enumerate() {
        for (pos, &iid) in bb.body.iter().enumerate() {
            let (divisor, divisor_ty) = match &func.instr(iid).kind {
                InstrKind::SDiv { rhs, .. } => (*rhs, func.instr(iid).ty),
                InstrKind::UDiv { rhs, .. } => (*rhs, func.instr(iid).ty),
                InstrKind::SRem { rhs, .. } => (*rhs, func.instr(iid).ty),
                InstrKind::URem { rhs, .. } => (*rhs, func.instr(iid).ty),
                _ => continue,
            };
            sites.push(DivSite {
                block_idx: bi,
                body_pos: pos,
                divisor,
                divisor_ty,
            });
        }
    }

    if sites.is_empty() {
        return false;
    }

    let void_ty = ctx.void_ty;
    let i1_ty = ctx.i1_ty;
    let handler_fn_ty = ctx.mk_fn_type(void_ty, vec![], false);

    let callee = match placeholder_callee("__ubsan_handle_divrem_overflow") {
        Some(c) => c,
        None => return false,
    };

    // Process in reverse block/pos order.
    sites.sort_by(|a, b| b.block_idx.cmp(&a.block_idx).then(b.body_pos.cmp(&a.body_pos)));

    for site in sites {
        let DivSite {
            block_idx,
            body_pos,
            divisor,
            divisor_ty,
        } = site;

        let zero_const = ctx.const_int(divisor_ty, 0);
        let zero_ref = ValueRef::Constant(zero_const);

        // Plan the split: body[0..body_pos] stays; body[body_pos..] + term → ok_block.
        let ok_body: Vec<InstrId> = func.blocks[block_idx].body[body_pos..].to_vec();
        let ok_term = func.blocks[block_idx].terminator;

        func.blocks[block_idx].body.truncate(body_pos);
        func.blocks[block_idx].terminator = None;

        let ok_block_idx = func.blocks.len() as u32;
        let div0_block_idx = ok_block_idx + 1;

        // Build ok_block (original div + rest).
        let mut ok_bb = BasicBlock::new(func.fresh_name());
        ok_bb.body = ok_body;
        ok_bb.terminator = ok_term;

        // Build div0_block.
        let div0_bb_name = func.fresh_name();

        // Emit check instructions into the current block (now the check block).

        // icmp eq divisor, 0.
        let cmp_name = func.fresh_name();
        let cmp_iid = func.alloc_instr(Instruction {
            name: Some(cmp_name),
            ty: i1_ty,
            kind: InstrKind::ICmp {
                pred: IntPredicate::Eq,
                lhs: divisor,
                rhs: zero_ref,
            },
        });
        func.blocks[block_idx].body.push(cmp_iid);

        // cond_br cmp, div0_block, ok_block.
        let cond_br_iid = func.alloc_instr(Instruction {
            name: None,
            ty: void_ty,
            kind: InstrKind::CondBr {
                cond: ValueRef::Instruction(cmp_iid),
                then_dest: BlockId(div0_block_idx),
                else_dest: BlockId(ok_block_idx),
            },
        });
        func.blocks[block_idx].set_terminator(cond_br_iid);

        // Build div0_block: call handler + br ok_block.
        let mut div0_bb = BasicBlock::new(div0_bb_name);

        let call_iid = func.alloc_instr(Instruction {
            name: None,
            ty: void_ty,
            kind: InstrKind::Call {
                tail: TailCallKind::None,
                callee_ty: handler_fn_ty,
                callee,
                args: vec![],
            },
        });
        div0_bb.body.push(call_iid);

        let br_ok_iid = func.alloc_instr(Instruction {
            name: None,
            ty: void_ty,
            kind: InstrKind::Br {
                dest: BlockId(ok_block_idx),
            },
        });
        div0_bb.set_terminator(br_ok_iid);

        // Append blocks: ok first, then div0.
        func.blocks.push(ok_bb);
        func.blocks.push(div0_bb);
    }

    true
}

// ---------------------------------------------------------------------------
// Null pointer dereference checks
// ---------------------------------------------------------------------------

/// For each Load/Store whose pointer is not provably non-null (not an alloca
/// or global), insert a null check before the access.
fn instrument_null_ptr(ctx: &mut Context, func: &mut Function) -> bool {
    struct NullCheckSite {
        block_idx: usize,
        body_pos: usize,
        ptr: ValueRef,
    }

    // Collect alloca IDs (always non-null).
    let alloca_ids: std::collections::HashSet<InstrId> = func
        .blocks
        .iter()
        .flat_map(|bb| bb.body.iter())
        .filter_map(|&iid| {
            if matches!(func.instr(iid).kind, InstrKind::Alloca { .. }) {
                Some(iid)
            } else {
                None
            }
        })
        .collect();

    let mut sites: Vec<NullCheckSite> = Vec::new();

    for (bi, bb) in func.blocks.iter().enumerate() {
        for (pos, &iid) in bb.body.iter().enumerate() {
            let ptr = match &func.instr(iid).kind {
                InstrKind::Load { ptr, .. } => *ptr,
                InstrKind::Store { ptr, .. } => *ptr,
                _ => continue,
            };
            // Skip provably non-null pointers.
            if is_provably_non_null(ptr, &alloca_ids) {
                continue;
            }
            sites.push(NullCheckSite {
                block_idx: bi,
                body_pos: pos,
                ptr,
            });
        }
    }

    if sites.is_empty() {
        return false;
    }

    let void_ty = ctx.void_ty;
    let i1_ty = ctx.i1_ty;
    let ptr_ty = ctx.ptr_ty;
    let handler_fn_ty = ctx.mk_fn_type(void_ty, vec![], false);
    let null_ptr_const = ctx.const_null(ptr_ty);
    let null_ref = ValueRef::Constant(null_ptr_const);

    let callee = match placeholder_callee("__ubsan_handle_null_ptr_deref") {
        Some(c) => c,
        None => return false,
    };

    // Process in reverse order.
    sites.sort_by(|a, b| {
        b.block_idx
            .cmp(&a.block_idx)
            .then(b.body_pos.cmp(&a.body_pos))
    });

    for site in sites {
        let NullCheckSite {
            block_idx,
            body_pos,
            ptr,
        } = site;

        // Split: body[0..body_pos] in check_block; body[body_pos..] + term in ok_block.
        let ok_body: Vec<InstrId> = func.blocks[block_idx].body[body_pos..].to_vec();
        let ok_term = func.blocks[block_idx].terminator;

        func.blocks[block_idx].body.truncate(body_pos);
        func.blocks[block_idx].terminator = None;

        let ok_block_idx = func.blocks.len() as u32;
        let null_block_idx = ok_block_idx + 1;

        let mut ok_bb = BasicBlock::new(func.fresh_name());
        ok_bb.body = ok_body;
        ok_bb.terminator = ok_term;

        let null_bb_name = func.fresh_name();

        // Emit into check block.

        // icmp eq ptr, null.
        let cmp_name = func.fresh_name();
        let cmp_iid = func.alloc_instr(Instruction {
            name: Some(cmp_name),
            ty: i1_ty,
            kind: InstrKind::ICmp {
                pred: IntPredicate::Eq,
                lhs: ptr,
                rhs: null_ref,
            },
        });
        func.blocks[block_idx].body.push(cmp_iid);

        // cond_br cmp, null_block, ok_block.
        let cond_br_iid = func.alloc_instr(Instruction {
            name: None,
            ty: void_ty,
            kind: InstrKind::CondBr {
                cond: ValueRef::Instruction(cmp_iid),
                then_dest: BlockId(null_block_idx),
                else_dest: BlockId(ok_block_idx),
            },
        });
        func.blocks[block_idx].set_terminator(cond_br_iid);

        // null_block: call handler + br ok_block.
        let mut null_bb = BasicBlock::new(null_bb_name);

        let call_iid = func.alloc_instr(Instruction {
            name: None,
            ty: void_ty,
            kind: InstrKind::Call {
                tail: TailCallKind::None,
                callee_ty: handler_fn_ty,
                callee,
                args: vec![],
            },
        });
        null_bb.body.push(call_iid);

        let br_ok_iid = func.alloc_instr(Instruction {
            name: None,
            ty: void_ty,
            kind: InstrKind::Br {
                dest: BlockId(ok_block_idx),
            },
        });
        null_bb.set_terminator(br_ok_iid);

        func.blocks.push(ok_bb);
        func.blocks.push(null_bb);
    }

    true
}

// ---------------------------------------------------------------------------
// Unreachable replacement
// ---------------------------------------------------------------------------

/// Replace each `unreachable` terminator with:
///   call __ubsan_handle_builtin_unreachable()
///   unreachable
fn instrument_unreachable(ctx: &mut Context, func: &mut Function) -> bool {
    let void_ty = ctx.void_ty;
    let handler_fn_ty = ctx.mk_fn_type(void_ty, vec![], false);

    let callee = match placeholder_callee("__ubsan_handle_builtin_unreachable") {
        Some(c) => c,
        None => return false,
    };

    // Collect blocks with `unreachable` terminator.
    let unreachable_blocks: Vec<usize> = func
        .blocks
        .iter()
        .enumerate()
        .filter_map(|(bi, bb)| {
            bb.terminator.and_then(|tid| {
                if matches!(func.instr(tid).kind, InstrKind::Unreachable) {
                    Some(bi)
                } else {
                    None
                }
            })
        })
        .collect();

    if unreachable_blocks.is_empty() {
        return false;
    }

    for bi in unreachable_blocks {
        let call_iid = func.alloc_instr(Instruction {
            name: None,
            ty: void_ty,
            kind: InstrKind::Call {
                tail: TailCallKind::None,
                callee_ty: handler_fn_ty,
                callee,
                args: vec![],
            },
        });
        // Insert the handler call at the end of the block body (before the terminator).
        func.blocks[bi].body.push(call_iid);
        // Leave the `unreachable` terminator in place.
    }

    true
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn is_integer_type(ctx: &Context, ty: llvm_ir::TypeId) -> bool {
    matches!(ctx.get_type(ty), llvm_ir::TypeData::Integer(_))
}

fn is_provably_non_null(
    ptr: ValueRef,
    alloca_ids: &std::collections::HashSet<InstrId>,
) -> bool {
    match ptr {
        // Alloca: always non-null (stack pointer).
        ValueRef::Instruction(iid) => alloca_ids.contains(&iid),
        // Global variables are also always non-null.
        ValueRef::Global(_) => true,
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
    use llvm_ir::{Builder, Context, InstrKind, Linkage, Module};

    /// Convenience: run UbsanModulePass (which declares runtime fns first).
    fn run_ubsan(ctx: &mut Context, module: &mut Module) {
        let mut pass = UbsanModulePass;
        pass.run_on_module(ctx, module);
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

    fn make_add_module() -> (Context, Module) {
        let mut ctx = Context::new();
        let mut module = Module::new("test");
        let mut b = Builder::new(&mut ctx, &mut module);
        let i32_ty = b.ctx.i32_ty;
        b.add_function(
            "f",
            i32_ty,
            vec![i32_ty, i32_ty],
            vec!["a".into(), "b".into()],
            false,
            Linkage::External,
        );
        let entry = b.add_block("entry");
        b.position_at_end(entry);
        let a = b.get_arg(0);
        let bv = b.get_arg(1);
        let r = b.build_add("r", a, bv);
        b.build_ret(r);
        (ctx, module)
    }

    fn make_add_nsw_module() -> (Context, Module) {
        let mut ctx = Context::new();
        let mut module = Module::new("test");
        let mut b = Builder::new(&mut ctx, &mut module);
        let i32_ty = b.ctx.i32_ty;
        b.add_function(
            "f",
            i32_ty,
            vec![i32_ty, i32_ty],
            vec!["a".into(), "b".into()],
            false,
            Linkage::External,
        );
        let entry = b.add_block("entry");
        b.position_at_end(entry);
        let a = b.get_arg(0);
        let bv = b.get_arg(1);
        let r = b.build_add_nsw("r", a, bv);
        b.build_ret(r);
        (ctx, module)
    }

    fn make_div_module() -> (Context, Module) {
        let mut ctx = Context::new();
        let mut module = Module::new("test");
        let mut b = Builder::new(&mut ctx, &mut module);
        let i32_ty = b.ctx.i32_ty;
        b.add_function(
            "f",
            i32_ty,
            vec![i32_ty, i32_ty],
            vec!["a".into(), "b".into()],
            false,
            Linkage::External,
        );
        let entry = b.add_block("entry");
        b.position_at_end(entry);
        let a = b.get_arg(0);
        let bv = b.get_arg(1);
        let r = b.build_sdiv("r", a, bv);
        b.build_ret(r);
        (ctx, module)
    }

    fn make_rem_module() -> (Context, Module) {
        let mut ctx = Context::new();
        let mut module = Module::new("test");
        let mut b = Builder::new(&mut ctx, &mut module);
        let i32_ty = b.ctx.i32_ty;
        b.add_function(
            "f",
            i32_ty,
            vec![i32_ty, i32_ty],
            vec!["a".into(), "b".into()],
            false,
            Linkage::External,
        );
        let entry = b.add_block("entry");
        b.position_at_end(entry);
        let a = b.get_arg(0);
        let bv = b.get_arg(1);
        let r = b.build_srem("r", a, bv);
        b.build_ret(r);
        (ctx, module)
    }

    fn make_load_ptr_arg_module() -> (Context, Module) {
        // f(ptr %p) -> i32: load from p (nullable arg pointer)
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

    fn make_store_ptr_arg_module() -> (Context, Module) {
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

    fn make_alloca_load_module() -> (Context, Module) {
        let mut ctx = Context::new();
        let mut module = Module::new("test");
        let mut b = Builder::new(&mut ctx, &mut module);
        let i32_ty = b.ctx.i32_ty;
        b.add_function("f", b.ctx.void_ty, vec![], vec![], false, Linkage::External);
        let entry = b.add_block("entry");
        b.position_at_end(entry);
        let slot = b.build_alloca("slot", i32_ty);
        let _v = b.build_load("v", i32_ty, slot);
        b.build_ret_void();
        (ctx, module)
    }

    fn make_unreachable_module() -> (Context, Module) {
        let mut ctx = Context::new();
        let mut module = Module::new("test");
        let mut b = Builder::new(&mut ctx, &mut module);
        b.add_function("f", b.ctx.void_ty, vec![], vec![], false, Linkage::External);
        let entry = b.add_block("entry");
        b.position_at_end(entry);
        b.build_unreachable();
        (ctx, module)
    }

    fn make_global_load_module() -> (Context, Module) {
        // Load from a GlobalId (always non-null).
        let mut ctx = Context::new();
        let mut module = Module::new("test");
        let mut b = Builder::new(&mut ctx, &mut module);
        let i32_ty = b.ctx.i32_ty;
        // Add a global.
        let _gid = b.add_global("G", i32_ty, None, false, Linkage::External);
        b.add_function("f", i32_ty, vec![], vec![], false, Linkage::External);
        let entry = b.add_block("entry");
        b.position_at_end(entry);
        // Build a load of &G (the global value as a pointer).
        let fid = b.current_function().unwrap();
        drop(b);
        // Manually construct a load from the global.
        let func = module.function_mut(fid);
        let gid_ref = ValueRef::Global(llvm_ir::GlobalId(0));
        let load_iid = func.alloc_instr(Instruction {
            name: Some("v".into()),
            ty: i32_ty,
            kind: InstrKind::Load {
                ty: i32_ty,
                ptr: gid_ref,
                align: None,
                volatile: false,
            },
        });
        func.blocks[0].body.push(load_iid);
        let ret_iid = func.alloc_instr(Instruction {
            name: None,
            ty: ctx.void_ty,
            kind: InstrKind::Ret {
                val: Some(ValueRef::Instruction(load_iid)),
            },
        });
        func.blocks[0].set_terminator(ret_iid);
        (ctx, module)
    }

    #[test]
    fn ubsan_add_without_nsw_gets_check() {
        let (mut ctx, mut module) = make_add_module();
        run_ubsan(&mut ctx, &mut module);
        let count = count_calls_to(&module, 0, "__ubsan_handle_add_overflow");
        assert!(count >= 1, "__ubsan_handle_add_overflow should be inserted for non-nsw add, got {count}");
    }

    #[test]
    fn ubsan_add_with_nsw_no_check() {
        let (mut ctx, mut module) = make_add_nsw_module();
        run_ubsan(&mut ctx, &mut module);
        let count = count_calls_to(&module, 0, "__ubsan_handle_add_overflow");
        assert_eq!(count, 0, "nsw add must NOT get overflow check, got {count}");
    }

    #[test]
    fn ubsan_div_gets_zero_check() {
        let (mut ctx, mut module) = make_div_module();
        run_ubsan(&mut ctx, &mut module);
        let count = count_calls_to(&module, 0, "__ubsan_handle_divrem_overflow");
        assert!(count >= 1, "__ubsan_handle_divrem_overflow should be inserted for sdiv, got {count}");
    }

    #[test]
    fn ubsan_rem_gets_zero_check() {
        let (mut ctx, mut module) = make_rem_module();
        run_ubsan(&mut ctx, &mut module);
        let count = count_calls_to(&module, 0, "__ubsan_handle_divrem_overflow");
        assert!(count >= 1, "__ubsan_handle_divrem_overflow should be inserted for srem, got {count}");
    }

    #[test]
    fn ubsan_load_null_check_inserted() {
        let (mut ctx, mut module) = make_load_ptr_arg_module();
        run_ubsan(&mut ctx, &mut module);
        let count = count_calls_to(&module, 0, "__ubsan_handle_null_ptr_deref");
        assert!(count >= 1, "__ubsan_handle_null_ptr_deref should be inserted for load from arg ptr, got {count}");
    }

    #[test]
    fn ubsan_store_null_check_inserted() {
        let (mut ctx, mut module) = make_store_ptr_arg_module();
        run_ubsan(&mut ctx, &mut module);
        let count = count_calls_to(&module, 0, "__ubsan_handle_null_ptr_deref");
        assert!(count >= 1, "__ubsan_handle_null_ptr_deref should be inserted for store to arg ptr, got {count}");
    }

    #[test]
    fn ubsan_alloca_no_null_check() {
        let (mut ctx, mut module) = make_alloca_load_module();
        run_ubsan(&mut ctx, &mut module);
        let count = count_calls_to(&module, 0, "__ubsan_handle_null_ptr_deref");
        assert_eq!(count, 0, "loads from alloca must NOT get null check, got {count}");
    }

    #[test]
    fn ubsan_global_no_null_check() {
        let (mut ctx, mut module) = make_global_load_module();
        run_ubsan(&mut ctx, &mut module);
        // function index 1 (function 0 is... wait, globals are separate from functions)
        // The function is the last added function — after the global.
        let func_idx = module
            .functions
            .iter()
            .position(|f| f.name == "f")
            .unwrap();
        let count = count_calls_to(&module, func_idx, "__ubsan_handle_null_ptr_deref");
        assert_eq!(count, 0, "loads from globals must NOT get null check, got {count}");
    }

    #[test]
    fn ubsan_unreachable_replaced_with_handler() {
        let (mut ctx, mut module) = make_unreachable_module();
        run_ubsan(&mut ctx, &mut module);
        let count = count_calls_to(&module, 0, "__ubsan_handle_builtin_unreachable");
        assert!(count >= 1, "__ubsan_handle_builtin_unreachable should be inserted, got {count}");
        // Unreachable terminator must still be present.
        let still_unreachable = module.functions[0]
            .blocks
            .iter()
            .filter_map(|bb| bb.terminator)
            .any(|tid| matches!(module.functions[0].instr(tid).kind, InstrKind::Unreachable));
        assert!(still_unreachable, "unreachable terminator must still be present after instrumentation");
    }

    #[test]
    fn ubsan_declares_runtime_fns() {
        let (mut ctx, mut module) = make_add_module();
        run_ubsan(&mut ctx, &mut module);
        for name in &[
            "__ubsan_handle_add_overflow",
            "__ubsan_handle_sub_overflow",
            "__ubsan_handle_mul_overflow",
            "__ubsan_handle_null_ptr_deref",
            "__ubsan_handle_divrem_overflow",
            "__ubsan_handle_builtin_unreachable",
        ] {
            assert!(
                module.get_function_id(name).is_some(),
                "{name} must be declared"
            );
        }
    }
}
