use llvm_ir::{BlockId, ConstantData, Context, Function, InstrId, InstrKind, ValueRef};

use crate::pass::FunctionPass;

/// Jump-threading pass.
///
/// Three transformations are applied in a single sweep:
///
/// 1. **Constant-condition folding** — `condbr i1 true, %then, %else` →
///    `br %then` (and vice-versa for `false`).
///
/// 2. **Phi-based threading** — When a `condbr` condition is a `phi` whose
///    incoming values are all compile-time constants, every predecessor that
///    supplies a known constant is redirected to bypass the join block and
///    jump directly to the appropriate successor.
///
/// 3. **Empty-block bypass** — When a `condbr` branches to a block that
///    contains no body instructions and ends in an unconditional `br`, the
///    `condbr` destination is redirected to that block's target.  This
///    eliminates trivially-empty intermediate blocks.
#[derive(Default)]
pub struct JumpThreading;

impl FunctionPass for JumpThreading {
    fn run_on_function(&mut self, ctx: &mut Context, func: &mut Function) -> bool {
        let mut changed = false;

        // ── pass 1: constant-condbr folding ──────────────────────────────────
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

        // ── pass 2: phi-based threading ───────────────────────────────────────
        // For each block whose terminator is `condbr %phi_result, %then, %else`
        // where %phi_result is a phi with constant incoming values — redirect
        // each predecessor that provides a known constant directly to the
        // appropriate branch target.
        //
        // We iterate by index to avoid borrow issues; modifications are written
        // back in-place after each predecessor inspection.
        for bidx in 0..func.blocks.len() {
            let Some(tid) = func.blocks[bidx].terminator else {
                continue;
            };

            // Extract the condbr info.
            let (cond_vref, then_dest, else_dest) = match &func.instr(tid).kind {
                InstrKind::CondBr {
                    cond,
                    then_dest,
                    else_dest,
                } => (*cond, *then_dest, *else_dest),
                _ => continue,
            };

            // Check that the condition is a phi in this block.
            let phi_iid = match cond_vref {
                ValueRef::Instruction(iid) => iid,
                _ => continue,
            };
            // Confirm the phi instruction belongs to the current block.
            if !func.blocks[bidx].body.contains(&phi_iid) {
                continue;
            }

            // Collect the phi's incoming pairs.
            let incoming: Vec<(ValueRef, BlockId)> = match &func.instr(phi_iid).kind {
                InstrKind::Phi { incoming, .. } => incoming.clone(),
                _ => continue,
            };

            // For each incoming edge where the value is a known constant,
            // redirect the predecessor's branch to the appropriate successor.
            let join_bid = BlockId(bidx as u32);
            for (val, src_bid) in &incoming {
                let Some(taken) = const_bool_like(ctx, *val) else {
                    continue;
                };
                let new_target = if taken { then_dest } else { else_dest };

                // Find the predecessor's terminator and patch it.
                let src_idx = src_bid.0 as usize;
                let Some(src_tid) = func.blocks[src_idx].terminator else {
                    continue;
                };
                let src_term = func.instr(src_tid).kind.clone();
                let patched = match src_term {
                    InstrKind::Br { dest } if dest == join_bid => {
                        Some(InstrKind::Br { dest: new_target })
                    }
                    InstrKind::CondBr {
                        cond,
                        then_dest: td,
                        else_dest: ed,
                    } => {
                        let new_td = if td == join_bid { new_target } else { td };
                        let new_ed = if ed == join_bid { new_target } else { ed };
                        if new_td != td || new_ed != ed {
                            Some(InstrKind::CondBr {
                                cond,
                                then_dest: new_td,
                                else_dest: new_ed,
                            })
                        } else {
                            None
                        }
                    }
                    _ => None,
                };
                if let Some(kind) = patched {
                    func.instr_mut(src_tid).kind = kind;
                    changed = true;
                }
            }
        }

        // ── pass 3: empty-block bypass ────────────────────────────────────────
        // If a `condbr` target is a block with no body instructions and an
        // unconditional `br` as its only terminator, skip straight to that
        // block's destination.  Repeat until no more simplifications apply.
        let mut local_changed = true;
        while local_changed {
            local_changed = false;
            for bidx in 0..func.blocks.len() {
                let Some(tid) = func.blocks[bidx].terminator else {
                    continue;
                };
                // Work on a clone to avoid simultaneous borrows.
                let term_kind = func.instr(tid).kind.clone();
                let (cond, then_dest, else_dest) = match term_kind {
                    InstrKind::CondBr {
                        cond,
                        then_dest,
                        else_dest,
                    } => (cond, then_dest, else_dest),
                    _ => continue,
                };

                let new_then = bypass_if_empty(func, then_dest);
                let new_else = bypass_if_empty(func, else_dest);

                if new_then != then_dest || new_else != else_dest {
                    // Before redirecting the edge, fix up the destination's phi
                    // nodes.  Bypassing `bidx → empty → succ` means `succ` now
                    // has a direct edge from `bidx`; every phi entry that
                    // attributed a value to the empty block must also attribute
                    // that value to `bidx`.  Without this, the empty block is
                    // later pruned as unreachable and the phi loses an incoming
                    // value entirely — silently miscompiling loops whose header
                    // phis carry a preheader value (see jit_fibonacci).
                    if new_then != then_dest {
                        add_phi_edge_for_bypass(func, then_dest, new_then, BlockId(bidx as u32));
                    }
                    if new_else != else_dest {
                        add_phi_edge_for_bypass(func, else_dest, new_else, BlockId(bidx as u32));
                    }
                    func.instr_mut(tid).kind = InstrKind::CondBr {
                        cond,
                        then_dest: new_then,
                        else_dest: new_else,
                    };
                    changed = true;
                    local_changed = true;
                }
            }
        }

        changed
    }

    fn name(&self) -> &'static str {
        "jump-threading"
    }
}

/// After bypassing an empty block `empty` (so that predecessor `new_pred` now
/// branches directly to `succ`), update `succ`'s phi nodes so that every
/// incoming entry attributed to `empty` is also attributed to `new_pred`.
///
/// The `empty` entry itself is left in place: if `empty` remains reachable
/// from other predecessors the entry is still required, and if it becomes
/// unreachable a later dead-block prune drops it.  We skip adding a duplicate
/// when `succ` already lists `new_pred` as an incoming edge (the degenerate
/// case where `new_pred` reaches `succ` via two edges).
fn add_phi_edge_for_bypass(func: &mut Function, empty: BlockId, succ: BlockId, new_pred: BlockId) {
    // `empty` must itself be a pure pass-through (no body) for this to be a
    // value-preserving bypass; `bypass_if_empty` already guaranteed that.
    let body = func.blocks[succ.0 as usize].body.clone();
    for iid in body {
        let (ty, incoming) = match &func.instr(iid).kind {
            InstrKind::Phi { ty, incoming } => (*ty, incoming.clone()),
            _ => continue,
        };
        // Value flowing through `empty`, and whether `new_pred` is already listed.
        let mut via_empty: Option<ValueRef> = None;
        let mut has_new_pred = false;
        for (v, b) in &incoming {
            if *b == empty {
                via_empty = Some(*v);
            }
            if *b == new_pred {
                has_new_pred = true;
            }
        }
        if let Some(v) = via_empty {
            if !has_new_pred {
                let mut new_incoming = incoming;
                new_incoming.push((v, new_pred));
                func.instr_mut(iid).kind = InstrKind::Phi {
                    ty,
                    incoming: new_incoming,
                };
            }
        }
    }
}

/// If `bid` is an empty block (no body instructions) that ends in an
/// unconditional `br`, return the branch target; otherwise return `bid`.
fn bypass_if_empty(func: &Function, bid: BlockId) -> BlockId {
    let block = &func.blocks[bid.0 as usize];
    if !block.body.is_empty() {
        return bid;
    }
    let Some(tid) = block.terminator else {
        return bid;
    };
    match &func.instr(tid).kind {
        InstrKind::Br { dest } => *dest,
        _ => bid,
    }
}

/// Returns the InstrId of the phi instruction if `v` is `ValueRef::Instruction`
/// and that instruction is a phi that lives in the block at index `bidx`.
#[allow(dead_code)]
fn phi_iid_in_block(func: &Function, bidx: usize, v: ValueRef) -> Option<InstrId> {
    let iid = match v {
        ValueRef::Instruction(i) => i,
        _ => return None,
    };
    if !func.blocks[bidx].body.contains(&iid) {
        return None;
    }
    match &func.instr(iid).kind {
        InstrKind::Phi { .. } => Some(iid),
        _ => None,
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

        let term = func.blocks[entry.0 as usize]
            .terminator
            .expect("terminator");
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

    /// phi_threading_true_branch:
    ///
    /// pred_true:
    ///   br %join
    ///
    /// pred_false:
    ///   br %join
    ///
    /// join:
    ///   %c = phi i1 [true, %pred_true], [false, %pred_false]
    ///   condbr %c, %then, %else
    ///
    /// After threading:
    ///   pred_true  → %then  (was %join)
    ///   pred_false → %else  (was %join)
    #[test]
    fn phi_threading_true_branch() {
        let mut ctx = Context::new();
        let mut module = Module::new("jt_phi_true");
        let mut b = Builder::new(&mut ctx, &mut module);
        b.add_function("f", b.ctx.void_ty, vec![], vec![], false, Linkage::External);

        let pred_true = b.add_block("pred_true");
        let pred_false = b.add_block("pred_false");
        let join = b.add_block("join");
        let then_b = b.add_block("then");
        let else_b = b.add_block("else");

        b.position_at_end(pred_true);
        b.build_br(join);

        b.position_at_end(pred_false);
        b.build_br(join);

        b.position_at_end(join);
        let c_true = b.const_bool(true);
        let c_false = b.const_bool(false);
        let phi = b.build_phi(
            "c",
            b.ctx.i1_ty,
            vec![(c_true, pred_true), (c_false, pred_false)],
        );
        b.build_cond_br(phi, then_b, else_b);

        b.position_at_end(then_b);
        b.build_ret_void();
        b.position_at_end(else_b);
        b.build_ret_void();

        let func = &mut module.functions[0];
        let mut pass = JumpThreading;
        assert!(pass.run_on_function(&mut ctx, func));

        // pred_true's terminator should now point to `then_b`.
        let pt_tid = func.blocks[pred_true.0 as usize]
            .terminator
            .expect("pred_true terminator");
        match &func.instr(pt_tid).kind {
            InstrKind::Br { dest } => assert_eq!(*dest, then_b, "pred_true should go to then"),
            other => panic!("expected Br, got {other:?}"),
        }

        // pred_false's terminator should now point to `else_b`.
        let pf_tid = func.blocks[pred_false.0 as usize]
            .terminator
            .expect("pred_false terminator");
        match &func.instr(pf_tid).kind {
            InstrKind::Br { dest } => assert_eq!(*dest, else_b, "pred_false should go to else"),
            other => panic!("expected Br, got {other:?}"),
        }
    }

    /// phi_threading_false_branch: the phi has [false, %pred_true],
    /// so pred_true should be redirected to else.
    #[test]
    fn phi_threading_false_branch() {
        let mut ctx = Context::new();
        let mut module = Module::new("jt_phi_false");
        let mut b = Builder::new(&mut ctx, &mut module);
        b.add_function("g", b.ctx.void_ty, vec![], vec![], false, Linkage::External);

        let pred_true = b.add_block("pred_true");
        let join = b.add_block("join");
        let then_b = b.add_block("then");
        let else_b = b.add_block("else");

        // Only one predecessor supplies false → it should go to else.
        b.position_at_end(pred_true);
        b.build_br(join);

        b.position_at_end(join);
        let c_false = b.const_bool(false);
        let phi = b.build_phi("c", b.ctx.i1_ty, vec![(c_false, pred_true)]);
        b.build_cond_br(phi, then_b, else_b);

        b.position_at_end(then_b);
        b.build_ret_void();
        b.position_at_end(else_b);
        b.build_ret_void();

        let func = &mut module.functions[0];
        let mut pass = JumpThreading;
        assert!(pass.run_on_function(&mut ctx, func));

        let pt_tid = func.blocks[pred_true.0 as usize]
            .terminator
            .expect("pred_true terminator");
        match &func.instr(pt_tid).kind {
            InstrKind::Br { dest } => {
                assert_eq!(*dest, else_b, "false phi → pred_true should go to else")
            }
            other => panic!("expected Br, got {other:?}"),
        }
    }

    /// empty_block_bypass: condbr whose `then` arm is an empty block that
    /// unconditionally branches to %real_then should be rewritten to jump
    /// directly to %real_then.
    #[test]
    fn empty_block_bypass() {
        let mut ctx = Context::new();
        let mut module = Module::new("jt_empty");
        let mut b = Builder::new(&mut ctx, &mut module);
        b.add_function(
            "h",
            b.ctx.void_ty,
            vec![b.ctx.i1_ty],
            vec!["cond".into()],
            false,
            Linkage::External,
        );

        let cond_arg = b.get_arg(0);
        let entry = b.add_block("entry");
        let empty = b.add_block("empty"); // will be empty — only br %real_then
        let real_then = b.add_block("real_then");
        let else_b = b.add_block("else");

        b.position_at_end(entry);
        b.build_cond_br(cond_arg, empty, else_b);

        // `empty` has no body instructions, just an unconditional br.
        b.position_at_end(empty);
        b.build_br(real_then);

        b.position_at_end(real_then);
        b.build_ret_void();
        b.position_at_end(else_b);
        b.build_ret_void();

        let func = &mut module.functions[0];
        let mut pass = JumpThreading;
        assert!(pass.run_on_function(&mut ctx, func));

        // entry's condbr `then` arm should now point directly to real_then.
        let entry_tid = func.blocks[entry.0 as usize]
            .terminator
            .expect("entry terminator");
        match &func.instr(entry_tid).kind {
            InstrKind::CondBr { then_dest, .. } => {
                assert_eq!(*then_dest, real_then, "should bypass empty block");
            }
            other => panic!("expected CondBr, got {other:?}"),
        }
    }

    /// leaves_nontrivial_phi_alone: when all phi incoming values are
    /// non-constant (function arguments), the pass must not touch the block.
    #[test]
    fn leaves_nontrivial_phi_alone() {
        let mut ctx = Context::new();
        let mut module = Module::new("jt_non_const_phi");
        let mut b = Builder::new(&mut ctx, &mut module);
        // Two i1 arguments.
        b.add_function(
            "k",
            b.ctx.void_ty,
            vec![b.ctx.i1_ty, b.ctx.i1_ty],
            vec!["a".into(), "b".into()],
            false,
            Linkage::External,
        );

        let arg_a = b.get_arg(0);
        let arg_b = b.get_arg(1);
        let pred_a = b.add_block("pred_a");
        let pred_b = b.add_block("pred_b");
        let join = b.add_block("join");
        let then_b = b.add_block("then");
        let else_b = b.add_block("else");

        b.position_at_end(pred_a);
        b.build_br(join);
        b.position_at_end(pred_b);
        b.build_br(join);

        b.position_at_end(join);
        // Both incoming values are arguments (not constants) → no threading.
        let phi = b.build_phi("c", b.ctx.i1_ty, vec![(arg_a, pred_a), (arg_b, pred_b)]);
        b.build_cond_br(phi, then_b, else_b);

        b.position_at_end(then_b);
        b.build_ret_void();
        b.position_at_end(else_b);
        b.build_ret_void();

        let func = &mut module.functions[0];
        let mut pass = JumpThreading;
        // The pass should not change anything — phi values are not constants.
        assert!(!pass.run_on_function(&mut ctx, func));
    }

    /// Regression for the `jit_fibonacci` miscompile (roadmap Milestone V /
    /// issue #381): when the empty preheader block (`loop_init`, just
    /// `br %loop_body`) is bypassed, the loop-header phis must keep their
    /// preheader incoming value — re-attributed to the bypassing predecessor
    /// (`entry`) — not silently lose it.
    ///
    /// Before the fix, JumpThreading redirected `entry → loop_body` without
    /// updating `loop_body`'s phis, leaving stale `[v, %loop_init]` entries
    /// that a later dead-block prune dropped, collapsing each 2-input loop phi
    /// to its back-edge value alone.
    #[test]
    fn empty_block_bypass_preserves_loop_header_phi_incoming() {
        use llvm_ir::InstrKind;
        use llvm_ir_parser::parser::parse;

        let src = r#"
define i32 @fib(i32 %n) {
entry:
  %cmp = icmp sle i32 %n, 1
  br i1 %cmp, label %base_case, label %loop_init
base_case:
  ret i32 %n
loop_init:
  br label %loop_body
loop_body:
  %a = phi i32 [ 0, %loop_init ], [ %b, %loop_body ]
  %b = phi i32 [ 1, %loop_init ], [ %next, %loop_body ]
  %next = add i32 %a, %b
  %cond = icmp slt i32 %next, %n
  br i1 %cond, label %loop_body, label %loop_exit
loop_exit:
  ret i32 %next
}
"#;
        let (mut ctx, mut module) = parse(src).expect("parse");
        let entry_idx = module.functions[0]
            .blocks
            .iter()
            .position(|b| b.name == "entry")
            .unwrap() as u32;
        let loop_init_idx = module.functions[0]
            .blocks
            .iter()
            .position(|b| b.name == "loop_init")
            .unwrap() as u32;

        let func = &mut module.functions[0];
        let mut pass = JumpThreading;
        pass.run_on_function(&mut ctx, func);

        // After bypass, every loop_body phi must still carry two incoming
        // values: the back-edge value, and the preheader value now attributed
        // either to `entry` (re-pointed) or still to `loop_init` (kept until a
        // later prune) — but never fewer than two distinct sources.
        let lb = func.blocks.iter().find(|b| b.name == "loop_body").unwrap();
        let mut checked = 0;
        for &iid in &lb.body {
            if let InstrKind::Phi { incoming, .. } = &func.instr(iid).kind {
                assert!(
                    incoming.len() >= 2,
                    "loop phi lost an incoming entry: {incoming:?}"
                );
                // The preheader value must be reachable from `entry` now that
                // `loop_init` is bypassed: either re-pointed to entry, or the
                // stale loop_init entry is still present (pruned later).
                let has_preheader_edge = incoming
                    .iter()
                    .any(|(_, b)| b.0 == entry_idx || b.0 == loop_init_idx);
                assert!(
                    has_preheader_edge,
                    "loop phi has no preheader edge: {incoming:?}"
                );
                checked += 1;
            }
        }
        assert_eq!(checked, 2, "expected two loop-header phis");
    }
}
