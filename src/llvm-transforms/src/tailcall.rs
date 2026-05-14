use llvm_ir::{Context, Function, InstrKind, TailCallKind, ValueRef};

use crate::pass::FunctionPass;

/// Marks eligible call sites in tail position as `tail`.
#[derive(Default)]
pub struct TailCallOpt;

impl FunctionPass for TailCallOpt {
    fn run_on_function(&mut self, _ctx: &mut Context, func: &mut Function) -> bool {
        let mut changed = false;
        for bidx in 0..func.blocks.len() {
            let block = &func.blocks[bidx];
            let Some(ret_id) = block.terminator else {
                continue;
            };
            let Some(&last_id) = block.body.last() else {
                continue;
            };

            let eligible = match (&func.instr(last_id).kind, &func.instr(ret_id).kind) {
                (
                    InstrKind::Call {
                        tail,
                        callee_ty: _,
                        callee: _,
                        args: _,
                    },
                    InstrKind::Ret { val },
                ) => {
                    if *tail != TailCallKind::None {
                        false
                    } else {
                        match val {
                            Some(ValueRef::Instruction(iid)) => *iid == last_id,
                            None => true,
                            _ => false,
                        }
                    }
                }
                _ => false,
            };

            if eligible {
                if let InstrKind::Call { tail, .. } = &mut func.instr_mut(last_id).kind {
                    *tail = TailCallKind::Tail;
                    changed = true;
                }
            }
        }
        changed
    }

    fn name(&self) -> &'static str {
        "tailcall-opt"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use llvm_ir::{Builder, Context, InstrKind, Linkage, Module, TailCallKind};

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
}
