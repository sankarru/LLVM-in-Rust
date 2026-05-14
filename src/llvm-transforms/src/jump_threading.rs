use llvm_ir::{ConstantData, Context, Function, InstrKind, ValueRef};

use crate::pass::FunctionPass;

/// A small, conservative jump-threading slice.
///
/// Today this pass only folds `condbr` where the condition is already a
/// compile-time integer constant to an unconditional `br`.
#[derive(Default)]
pub struct JumpThreading;

impl FunctionPass for JumpThreading {
    fn run_on_function(&mut self, ctx: &mut Context, func: &mut Function) -> bool {
        let mut changed = false;
        for bidx in 0..func.blocks.len() {
            let Some(tid) = func.blocks[bidx].terminator else {
                continue;
            };
            let replacement = match &func.instr(tid).kind {
                InstrKind::CondBr {
                    cond,
                    then_dest,
                    else_dest,
                } => match const_bool_like(ctx, *cond) {
                    Some(true) => Some(InstrKind::Br { dest: *then_dest }),
                    Some(false) => Some(InstrKind::Br { dest: *else_dest }),
                    None => None,
                },
                _ => None,
            };
            if let Some(kind) = replacement {
                func.instr_mut(tid).kind = kind;
                changed = true;
            }
        }
        changed
    }

    fn name(&self) -> &'static str {
        "jump-threading"
    }
}

fn const_bool_like(ctx: &Context, v: ValueRef) -> Option<bool> {
    let ValueRef::Constant(cid) = v else {
        return None;
    };
    match ctx.get_const(cid) {
        ConstantData::Int { val, .. } => Some(*val != 0),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use llvm_ir::{Builder, Context, InstrKind, Linkage, Module};

    #[test]
    fn folds_const_condbr_to_unconditional_branch() {
        let mut ctx = Context::new();
        let mut module = Module::new("jt");
        let mut b = Builder::new(&mut ctx, &mut module);
        b.add_function(
            "main",
            b.ctx.void_ty,
            vec![],
            vec![],
            false,
            Linkage::External,
        );

        let entry = b.add_block("entry");
        let then_b = b.add_block("then");
        let else_b = b.add_block("else");

        b.position_at_end(entry);
        let c1 = b.const_int(b.ctx.i1_ty, 1);
        b.build_cond_br(c1, then_b, else_b);

        b.position_at_end(then_b);
        b.build_ret_void();
        b.position_at_end(else_b);
        b.build_ret_void();

        let func = &mut module.functions[0];
        let mut pass = JumpThreading;
        assert!(pass.run_on_function(&mut ctx, func));

        let term = func.blocks[entry.0 as usize].terminator.expect("terminator");
        match &func.instr(term).kind {
            InstrKind::Br { dest } => assert_eq!(*dest, then_b),
            other => panic!("expected Br after threading, got {other:?}"),
        }
    }

    #[test]
    fn leaves_non_constant_condbr_unchanged() {
        let mut ctx = Context::new();
        let mut module = Module::new("jt2");
        let mut b = Builder::new(&mut ctx, &mut module);
        b.add_function(
            "main",
            b.ctx.void_ty,
            vec![b.ctx.i1_ty],
            vec!["c".into()],
            false,
            Linkage::External,
        );

        let arg = b.get_arg(0);
        let entry = b.add_block("entry");
        let then_b = b.add_block("then");
        let else_b = b.add_block("else");
        b.position_at_end(entry);
        b.build_cond_br(arg, then_b, else_b);
        b.position_at_end(then_b);
        b.build_ret_void();
        b.position_at_end(else_b);
        b.build_ret_void();

        let func = &mut module.functions[0];
        let mut pass = JumpThreading;
        assert!(!pass.run_on_function(&mut ctx, func));
    }
}
