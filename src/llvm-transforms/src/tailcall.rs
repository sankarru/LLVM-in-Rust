use llvm_ir::{Context, Function, InstrKind, Module, TailCallKind, ValueRef};

use crate::pass::{FunctionPass, ModulePass};

/// Marks eligible call sites in tail position as `tail` or `musttail`.
///
/// A call is in tail position when:
/// 1. It is the last non-terminator instruction in its block.
/// 2. The block's terminator is `ret void` **or** `ret %call_result`.
/// 3. The call's `TailCallKind` is currently `None` (we never override `NoTail`).
///
/// Additionally, if the callee is the **same function** as the one being
/// compiled (a direct self-recursive tail call), the kind is upgraded to
/// `MustTail` so that backends can emit a true loop-back jump.
#[derive(Default)]
pub struct TailCallOpt;

impl ModulePass for TailCallOpt {
    fn run_on_module(&mut self, ctx: &mut Context, module: &mut Module) -> bool {
        let mut changed = false;
        // Collect (function_index, function_name) pairs so we can look up
        // callees by GlobalId later.
        let name_by_idx: Vec<String> = module.functions.iter().map(|f| f.name.clone()).collect();

        for fidx in 0..module.functions.len() {
            if module.functions[fidx].is_declaration {
                continue;
            }
            let func_name = module.functions[fidx].name.clone();
            changed |= run_on_function_with_name(
                ctx,
                &mut module.functions[fidx],
                &func_name,
                &name_by_idx,
            );
        }
        changed
    }

    fn name(&self) -> &'static str {
        "tailcall-opt"
    }
}

/// Kept for use in unit tests that only have a single `Function` and no
/// `Module`.  In the normal pipeline the `ModulePass` impl is used instead,
/// which additionally detects self-recursive `MustTail` calls.
impl FunctionPass for TailCallOpt {
    fn run_on_function(&mut self, ctx: &mut Context, func: &mut Function) -> bool {
        // Without module context we cannot resolve callee names, so we fall
        // back to a conservative scan that never marks `MustTail`.
        run_on_function_with_name(ctx, func, &func.name.clone(), &[])
    }

    fn name(&self) -> &'static str {
        "tailcall-opt"
    }
}

/// Core per-function logic.  `func_name` is the name of the function being
/// scanned; `name_by_idx` maps `GlobalId(i)` → function name (may be empty
/// when called from the `FunctionPass` shim).
fn run_on_function_with_name(
    _ctx: &mut Context,
    func: &mut Function,
    func_name: &str,
    name_by_idx: &[String],
) -> bool {
    let mut changed = false;
    for bidx in 0..func.blocks.len() {
        let block = &func.blocks[bidx];
        let Some(ret_id) = block.terminator else {
            continue;
        };
        let Some(&last_id) = block.body.last() else {
            continue;
        };

        // Determine whether the call is in tail position and whether it is
        // self-recursive.
        let action = match (&func.instr(last_id).kind, &func.instr(ret_id).kind) {
            (
                InstrKind::Call {
                    tail,
                    callee,
                    callee_ty: _,
                    args: _,
                },
                InstrKind::Ret { val },
            ) => {
                if *tail != TailCallKind::None {
                    // Already annotated — leave it alone.
                    None
                } else {
                    let is_tail = match val {
                        Some(ValueRef::Instruction(iid)) => *iid == last_id,
                        None => true,
                        _ => false,
                    };
                    if !is_tail {
                        None
                    } else {
                        // Check for self-recursive call.
                        let is_self = match callee {
                            ValueRef::Global(gid) => name_by_idx
                                .get(gid.0 as usize)
                                .map(|n| n == func_name)
                                .unwrap_or(false),
                            _ => false,
                        };
                        Some(if is_self {
                            TailCallKind::MustTail
                        } else {
                            TailCallKind::Tail
                        })
                    }
                }
            }
            _ => None,
        };

        if let Some(kind) = action {
            if let InstrKind::Call { tail, .. } = &mut func.instr_mut(last_id).kind {
                *tail = kind;
                changed = true;
            }
        }
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;
    use llvm_ir::{Builder, Context, InstrKind, Linkage, Module, TailCallKind};

    // ── original test (kept unchanged) ────────────────────────────────────────

    #[test]
    fn marks_tail_position_call() {
        let mut ctx = Context::new();
        let mut module = Module::new("tco");
        let mut b = Builder::new(&mut ctx, &mut module);

        let callee_ty = b.ctx.mk_fn_type(b.ctx.i32_ty, vec![b.ctx.i32_ty], false);
        b.add_declaration("callee", b.ctx.i32_ty, vec![b.ctx.i32_ty], false);
        b.add_function(
            "main",
            b.ctx.i32_ty,
            vec![b.ctx.i32_ty],
            vec!["x".into()],
            false,
            Linkage::External,
        );

        let entry = b.add_block("entry");
        b.position_at_end(entry);
        let arg = b.get_arg(0);
        let call = b.build_call(
            "r",
            b.ctx.i32_ty,
            callee_ty,
            llvm_ir::ValueRef::Global(llvm_ir::context::GlobalId(0)),
            vec![arg],
        );
        b.build_ret(call);

        let func = &mut module.functions[1];
        let call_id = func.blocks[0].body[0];
        let mut pass = TailCallOpt;
        assert!(pass.run_on_function(&mut ctx, func));
        match &func.instr(call_id).kind {
            InstrKind::Call { tail, .. } => assert_eq!(*tail, TailCallKind::Tail),
            other => panic!("expected call, got {other:?}"),
        }
    }

    // ── new tests ─────────────────────────────────────────────────────────────

    /// A call followed by other instructions before `ret` is NOT a tail call.
    #[test]
    fn tail_call_not_marked_non_tail() {
        let mut ctx = Context::new();
        let mut module = Module::new("tco_non_tail");
        let mut b = Builder::new(&mut ctx, &mut module);

        let callee_ty = b.ctx.mk_fn_type(b.ctx.i32_ty, vec![], false);
        b.add_declaration("callee", b.ctx.i32_ty, vec![], false);
        b.add_function("f", b.ctx.i32_ty, vec![], vec![], false, Linkage::External);
        let entry = b.add_block("entry");
        b.position_at_end(entry);
        let call = b.build_call(
            "r",
            b.ctx.i32_ty,
            callee_ty,
            llvm_ir::ValueRef::Global(llvm_ir::context::GlobalId(0)),
            vec![],
        );
        // Add an instruction AFTER the call — it is no longer in tail position.
        let one = b.const_int(b.ctx.i32_ty, 1);
        let _add = b.build_add("sum", call, one);
        b.build_ret(call);

        let func = &mut module.functions[1];
        let call_id = func.blocks[0].body[0];
        let mut pass = TailCallOpt;
        // `call` is not the LAST body instruction (add is), so not tail.
        assert!(!pass.run_on_function(&mut ctx, func));
        match &func.instr(call_id).kind {
            InstrKind::Call { tail, .. } => assert_eq!(*tail, TailCallKind::None),
            other => panic!("expected call, got {other:?}"),
        }
    }

    /// Self-recursive tail call should be marked `MustTail` (requires module).
    #[test]
    fn self_recursive_tail_marked_musttail() {
        let mut ctx = Context::new();
        let mut module = Module::new("tco_musttail");
        let mut b = Builder::new(&mut ctx, &mut module);

        let fn_ty = b.ctx.mk_fn_type(b.ctx.i32_ty, vec![b.ctx.i32_ty], false);
        // Function "fact" calls itself recursively.
        b.add_function(
            "fact",
            b.ctx.i32_ty,
            vec![b.ctx.i32_ty],
            vec!["n".into()],
            false,
            Linkage::External,
        );
        let entry = b.add_block("entry");
        b.position_at_end(entry);
        let arg = b.get_arg(0);
        // Self-call: callee = GlobalId(0) = "fact" (the only function, idx 0).
        let call = b.build_call(
            "r",
            b.ctx.i32_ty,
            fn_ty,
            llvm_ir::ValueRef::Global(llvm_ir::context::GlobalId(0)),
            vec![arg],
        );
        b.build_ret(call);

        let call_id = module.functions[0].blocks[0].body[0];
        let mut pass = TailCallOpt;
        assert!(pass.run_on_module(&mut ctx, &mut module));
        match &module.functions[0].instr(call_id).kind {
            InstrKind::Call { tail, .. } => {
                assert_eq!(
                    *tail,
                    TailCallKind::MustTail,
                    "self-recursive call should be MustTail"
                )
            }
            other => panic!("expected call, got {other:?}"),
        }
    }

    /// `NoTail` attribute must never be overridden.
    #[test]
    fn notail_attribute_preserved() {
        let mut ctx = Context::new();
        let mut module = Module::new("tco_notail");
        let mut b = Builder::new(&mut ctx, &mut module);

        let callee_ty = b.ctx.mk_fn_type(b.ctx.i32_ty, vec![], false);
        b.add_declaration("callee", b.ctx.i32_ty, vec![], false);
        b.add_function("f", b.ctx.i32_ty, vec![], vec![], false, Linkage::External);
        let entry = b.add_block("entry");
        b.position_at_end(entry);
        let call = b.build_call(
            "r",
            b.ctx.i32_ty,
            callee_ty,
            llvm_ir::ValueRef::Global(llvm_ir::context::GlobalId(0)),
            vec![],
        );
        b.build_ret(call);

        // Manually mark it NoTail before running the pass.
        let call_id = module.functions[1].blocks[0].body[0];
        if let InstrKind::Call { tail, .. } = &mut module.functions[1].instr_mut(call_id).kind {
            *tail = TailCallKind::NoTail;
        }

        let mut pass = TailCallOpt;
        // The pass must not change anything.
        assert!(!pass.run_on_module(&mut ctx, &mut module));
        match &module.functions[1].instr(call_id).kind {
            InstrKind::Call { tail, .. } => {
                assert_eq!(*tail, TailCallKind::NoTail, "NoTail must be preserved")
            }
            other => panic!("expected call, got {other:?}"),
        }
    }

    /// Tail call in a void function — the return is `ret void`.
    #[test]
    fn void_tail_call_marked() {
        let mut ctx = Context::new();
        let mut module = Module::new("tco_void");
        let mut b = Builder::new(&mut ctx, &mut module);

        let callee_ty = b.ctx.mk_fn_type(b.ctx.void_ty, vec![], false);
        b.add_declaration("sink", b.ctx.void_ty, vec![], false);
        b.add_function(
            "wrapper",
            b.ctx.void_ty,
            vec![],
            vec![],
            false,
            Linkage::External,
        );
        let entry = b.add_block("entry");
        b.position_at_end(entry);
        // void call — result is not used; ret void follows.
        b.build_call(
            "c",
            b.ctx.void_ty,
            callee_ty,
            llvm_ir::ValueRef::Global(llvm_ir::context::GlobalId(0)),
            vec![],
        );
        b.build_ret_void();

        let call_id = module.functions[1].blocks[0].body[0];
        let mut pass = TailCallOpt;
        assert!(pass.run_on_module(&mut ctx, &mut module));
        match &module.functions[1].instr(call_id).kind {
            InstrKind::Call { tail, .. } => {
                assert_eq!(
                    *tail,
                    TailCallKind::Tail,
                    "void tail call should be marked Tail"
                )
            }
            other => panic!("expected call, got {other:?}"),
        }
    }

    /// Run the full O2 pipeline and confirm that eligible tail calls are marked.
    #[test]
    fn tail_call_in_pipeline() {
        use crate::pipeline::{build_pipeline, OptLevel};

        let mut ctx = Context::new();
        let mut module = Module::new("tco_pipeline");
        let mut b = Builder::new(&mut ctx, &mut module);

        let callee_ty = b.ctx.mk_fn_type(b.ctx.i32_ty, vec![b.ctx.i32_ty], false);
        b.add_declaration("callee", b.ctx.i32_ty, vec![b.ctx.i32_ty], false);
        b.add_function(
            "main",
            b.ctx.i32_ty,
            vec![b.ctx.i32_ty],
            vec!["x".into()],
            false,
            Linkage::External,
        );
        let entry = b.add_block("entry");
        b.position_at_end(entry);
        let arg = b.get_arg(0);
        let call = b.build_call(
            "r",
            b.ctx.i32_ty,
            callee_ty,
            llvm_ir::ValueRef::Global(llvm_ir::context::GlobalId(0)),
            vec![arg],
        );
        b.build_ret(call);

        let call_id = module.functions[1].blocks[0].body[0];

        let mut pm = build_pipeline(OptLevel::O2);
        pm.run_until_fixed_point(&mut ctx, &mut module, 8);

        match &module.functions[1].instr(call_id).kind {
            InstrKind::Call { tail, .. } => {
                assert_ne!(
                    *tail,
                    TailCallKind::None,
                    "O2 pipeline should mark tail call"
                );
            }
            other => panic!("expected call, got {other:?}"),
        }
    }
}
