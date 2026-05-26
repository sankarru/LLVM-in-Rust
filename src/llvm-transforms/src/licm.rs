//! Loop-Invariant Code Motion (LICM).
//!
//! This pass hoists loop-invariant instructions from loop bodies to loop
//! preheaders, reducing the work done per iteration.
//!
//! # Algorithm
//!
//! For each natural loop (innermost loops first):
//! 1. Find or create a **preheader** — a unique predecessor of the loop header
//!    that lies outside the loop body.  If multiple outside predecessors exist,
//!    a new preheader block is inserted that redirects them all.
//! 2. Repeatedly collect **loop-invariant instructions**: instructions where
//!    - every `ValueRef` operand is a constant, a function argument, or an
//!      instruction defined in a block that is *not* in the loop body, AND
//!    - the instruction is side-effect-free (pure arithmetic / cast / compare).
//!    - Phi nodes are never hoisted (their semantics depend on loop control flow).
//! 3. Move those instructions, in definition order, to the end of the preheader
//!    (before the preheader's unconditional branch to the header).
//! 4. Repeat until no more instructions can be hoisted in the current loop.
//!
//! # Correctness
//!
//! Only pure (effect-free) instructions are considered.  Instructions with
//! side effects (`Store`, `Load`, `Call`, `Alloca`, atomics, terminators) are
//! never hoisted.  This is conservative but always correct.

use crate::pass::FunctionPass;
use llvm_analysis::{Cfg, DomTree, LoopInfo};
use llvm_ir::{BasicBlock, BlockId, Context, Function, InstrId, InstrKind, Instruction, TypeId, ValueRef};
use std::collections::HashSet;

/// Loop-Invariant Code Motion pass.
pub struct Licm;

impl FunctionPass for Licm {
    fn name(&self) -> &'static str {
        "licm"
    }

    fn run_on_function(&mut self, _ctx: &mut Context, func: &mut Function) -> bool {
        if func.blocks.is_empty() {
            return false;
        }

        let cfg = Cfg::compute(func);
        let dom = DomTree::compute(func, &cfg);
        let li = LoopInfo::compute(func, &cfg, &dom);

        if li.loops().is_empty() {
            return false;
        }

        // When the function is compiled in strict IEEE-754 mode, no FP op can
        // be hoisted out of a loop regardless of per-instruction flags.
        let strictfp_mode = func.strictfp;

        // Process innermost loops first: sort by body length ascending.
        let mut loop_order: Vec<usize> = (0..li.loops().len()).collect();
        loop_order.sort_by_key(|&i| li.loops()[i].body.len());

        let mut any_changed = false;

        for loop_idx in loop_order {
            let header = li.loops()[loop_idx].header;
            let loop_body: HashSet<BlockId> =
                li.loops()[loop_idx].body.iter().copied().collect();

            // Find or create the preheader once per loop.
            let preheader = ensure_preheader(func, &cfg, header, &loop_body);

            // Iteratively hoist invariant instructions until stable.
            loop {
                let hoisted = hoist_one_round(func, &loop_body, preheader, strictfp_mode);
                if !hoisted {
                    break;
                }
                any_changed = true;
            }
        }

        any_changed
    }
}

// ---------------------------------------------------------------------------
// Invariance helpers
// ---------------------------------------------------------------------------

/// Returns true if `kind` is side-effect-free and eligible for hoisting
/// from a structural (non-FP) perspective.
///
/// Pure arithmetic, bitwise, comparison, cast, and select instructions are
/// eligible.  Phi nodes are excluded — their incoming-block semantics are
/// tied to loop control flow and cannot be hoisted.
fn is_hoist_safe(kind: &InstrKind) -> bool {
    matches!(
        kind,
        InstrKind::Add { .. }
            | InstrKind::Sub { .. }
            | InstrKind::Mul { .. }
            | InstrKind::UDiv { .. }
            | InstrKind::SDiv { .. }
            | InstrKind::URem { .. }
            | InstrKind::SRem { .. }
            | InstrKind::And { .. }
            | InstrKind::Or { .. }
            | InstrKind::Xor { .. }
            | InstrKind::Shl { .. }
            | InstrKind::LShr { .. }
            | InstrKind::AShr { .. }
            | InstrKind::FAdd { .. }
            | InstrKind::FSub { .. }
            | InstrKind::FMul { .. }
            | InstrKind::FDiv { .. }
            | InstrKind::FRem { .. }
            | InstrKind::FNeg { .. }
            | InstrKind::ICmp { .. }
            | InstrKind::FCmp { .. }
            | InstrKind::Trunc { .. }
            | InstrKind::ZExt { .. }
            | InstrKind::SExt { .. }
            | InstrKind::FPTrunc { .. }
            | InstrKind::FPExt { .. }
            | InstrKind::FPToUI { .. }
            | InstrKind::FPToSI { .. }
            | InstrKind::UIToFP { .. }
            | InstrKind::SIToFP { .. }
            | InstrKind::PtrToInt { .. }
            | InstrKind::IntToPtr { .. }
            | InstrKind::BitCast { .. }
            | InstrKind::AddrSpaceCast { .. }
            | InstrKind::Freeze { .. }
            | InstrKind::Select { .. }
            | InstrKind::ExtractValue { .. }
            | InstrKind::InsertValue { .. }
    )
}

/// Returns true if a floating-point instruction is safe to hoist out of a
/// loop.  IEEE-754 requires FP operations to execute in program order unless
/// the instruction carries `reassoc` or `fast` flags.
///
/// In strictfp mode (function-level attribute) ALL FP hoisting is blocked
/// regardless of per-instruction flags, because strict mode mandates exact
/// program-order semantics.
fn is_fp_hoist_safe(kind: &InstrKind, strictfp_mode: bool) -> bool {
    match kind {
        InstrKind::FAdd { flags, .. }
        | InstrKind::FSub { flags, .. }
        | InstrKind::FMul { flags, .. }
        | InstrKind::FDiv { flags, .. }
        | InstrKind::FRem { flags, .. }
        | InstrKind::FNeg { flags, .. } => {
            if strictfp_mode {
                false // strict mode: never hoist FP ops
            } else {
                flags.reassoc || flags.fast
            }
        }
        _ => true, // non-FP instructions not affected by this check
    }
}

/// Build a map from `InstrId` → the `BlockId` that contains it (all blocks).
fn build_instr_to_block(func: &Function) -> Vec<Option<BlockId>> {
    let n = func.instructions.len();
    let mut map: Vec<Option<BlockId>> = vec![None; n];
    for (bi, bb) in func.blocks.iter().enumerate() {
        let bid = BlockId(bi as u32);
        for &iid in &bb.body {
            if (iid.0 as usize) < n {
                map[iid.0 as usize] = Some(bid);
            }
        }
        if let Some(tid) = bb.terminator {
            if (tid.0 as usize) < n {
                map[tid.0 as usize] = Some(bid);
            }
        }
    }
    map
}

/// Returns true if `iid`'s instruction is loop-invariant with respect to
/// `loop_body`, given a precomputed `instr_to_block` mapping.
///
/// `strictfp_mode` gates the IEEE-754 hoisting rule for FP instructions.
fn is_loop_invariant(
    func: &Function,
    iid: InstrId,
    loop_body: &HashSet<BlockId>,
    instr_to_block: &[Option<BlockId>],
    strictfp_mode: bool,
) -> bool {
    let instr = func.instr(iid);
    if !is_hoist_safe(&instr.kind) {
        return false;
    }
    // IEEE-754 strictness check: FP instructions may not be hoisted unless
    // their flags explicitly allow reordering.
    if !is_fp_hoist_safe(&instr.kind, strictfp_mode) {
        return false;
    }
    for operand in instr.kind.operands() {
        match operand {
            // Constants, arguments, and globals are always loop-invariant.
            ValueRef::Constant(_) | ValueRef::Argument(_) | ValueRef::Global(_) => {}
            ValueRef::Instruction(def_iid) => {
                // Look up which block defines this instruction.
                let def_block = instr_to_block
                    .get(def_iid.0 as usize)
                    .and_then(|b| *b);
                match def_block {
                    // Defined outside loop — OK.
                    Some(b) if !loop_body.contains(&b) => {}
                    // Defined inside loop — not invariant.
                    Some(_) => return false,
                    // Not in any block yet (e.g. a just-hoisted instr) — treat
                    // conservatively as outside.
                    None => {}
                }
            }
        }
    }
    true
}

// ---------------------------------------------------------------------------
// Core hoisting
// ---------------------------------------------------------------------------

/// Perform one round of hoisting for the given loop.
///
/// Returns `true` if at least one instruction was moved into `preheader`.
fn hoist_one_round(
    func: &mut Function,
    loop_body: &HashSet<BlockId>,
    preheader: BlockId,
    strictfp_mode: bool,
) -> bool {
    let instr_to_block = build_instr_to_block(func);

    // Collect candidates: non-terminator instructions in loop body blocks
    // that are loop-invariant.  We build (src_block, iid) pairs in
    // deterministic program order so hoisted instructions preserve their
    // relative ordering.
    let mut candidates: Vec<(BlockId, InstrId)> = Vec::new();
    // Sort loop body for determinism.
    let mut sorted_body: Vec<BlockId> = loop_body.iter().copied().collect();
    sorted_body.sort_unstable_by_key(|b| b.0);

    for bid in sorted_body {
        let body_snapshot: Vec<InstrId> = func.blocks[bid.0 as usize].body.clone();
        for iid in body_snapshot {
            if is_loop_invariant(func, iid, loop_body, &instr_to_block, strictfp_mode) {
                candidates.push((bid, iid));
            }
        }
    }

    if candidates.is_empty() {
        return false;
    }

    // Move each candidate instruction from its source block into the
    // preheader body (appended before the terminator, i.e. end of body vec).
    for (src_bid, iid) in &candidates {
        func.blocks[src_bid.0 as usize].body.retain(|&x| x != *iid);
        func.blocks[preheader.0 as usize].body.push(*iid);
    }

    true
}

// ---------------------------------------------------------------------------
// Preheader management
// ---------------------------------------------------------------------------

/// Ensure the loop headed by `header` has a single preheader that lies
/// entirely outside `loop_body`.
///
/// **Reuse case**: if `header` has exactly one predecessor outside the loop
/// and that predecessor ends with an unconditional `Br` to `header`, we
/// return that predecessor as the preheader without modifying the CFG.
///
/// **Create case**: otherwise, we insert a new block `<header>.preheader`,
/// add `Br header` as its terminator, and redirect every outside predecessor
/// to branch to the new preheader instead.  Phi nodes in `header` that
/// reference outside predecessors are updated accordingly.
fn ensure_preheader(
    func: &mut Function,
    cfg: &Cfg,
    header: BlockId,
    loop_body: &HashSet<BlockId>,
) -> BlockId {
    // Collect predecessors outside the loop.
    let outside_preds: Vec<BlockId> = cfg
        .predecessors(header)
        .iter()
        .copied()
        .filter(|p| !loop_body.contains(p))
        .collect();

    // Fast path: already a single unconditional predecessor → reuse it.
    if outside_preds.len() == 1 {
        let pred = outside_preds[0];
        if let Some(tid) = func.blocks[pred.0 as usize].terminator {
            if matches!(func.instr(tid).kind, InstrKind::Br { dest } if dest == header) {
                return pred;
            }
        }
    }

    // Create a new preheader block.
    let preheader_name = format!("{}.preheader", func.blocks[header.0 as usize].name);
    let mut preheader_bb = BasicBlock::new(preheader_name);

    // Determine the void TypeId from an existing terminator instruction.
    let void_ty = find_void_ty(func);

    // Append an unconditional Br → header as the preheader terminator.
    let br_instr = Instruction {
        name: None,
        ty: void_ty,
        kind: InstrKind::Br { dest: header },
    };
    let br_id = InstrId(func.instructions.len() as u32);
    func.instructions.push(br_instr);
    preheader_bb.set_terminator(br_id);

    // Append the new block and record its id.
    let preheader_id = BlockId(func.blocks.len() as u32);
    func.blocks.push(preheader_bb);

    // Redirect all outside predecessors: replace branches to `header` with
    // branches to `preheader_id`.
    for &pred in &outside_preds {
        redirect_branch(func, pred, header, preheader_id);
    }

    // Update Phi nodes in the header: incoming edges from outside predecessors
    // now come from `preheader_id`.
    let header_body: Vec<InstrId> = func.blocks[header.0 as usize].body.clone();
    for iid in header_body {
        if let InstrKind::Phi { incoming, .. } =
            &mut func.instructions[iid.0 as usize].kind
        {
            for (_, pred_bid) in incoming.iter_mut() {
                if outside_preds.contains(pred_bid) {
                    *pred_bid = preheader_id;
                }
            }
        }
    }

    preheader_id
}

/// Find the TypeId used for void-typed terminator instructions in `func`.
/// Falls back to `TypeId(0)` (which is always void in our context).
fn find_void_ty(func: &Function) -> TypeId {
    for instr in &func.instructions {
        if instr.kind.is_terminator() {
            return instr.ty;
        }
    }
    TypeId(0)
}

/// Rewrite all branch targets equal to `old_target` in `block`'s terminator
/// to point to `new_target` instead.
fn redirect_branch(
    func: &mut Function,
    block: BlockId,
    old_target: BlockId,
    new_target: BlockId,
) {
    let tid = match func.blocks[block.0 as usize].terminator {
        Some(t) => t,
        None => return,
    };
    match &mut func.instructions[tid.0 as usize].kind {
        InstrKind::Br { dest } => {
            if *dest == old_target {
                *dest = new_target;
            }
        }
        InstrKind::CondBr {
            then_dest,
            else_dest,
            ..
        } => {
            if *then_dest == old_target {
                *then_dest = new_target;
            }
            if *else_dest == old_target {
                *else_dest = new_target;
            }
        }
        InstrKind::Switch { default, cases, .. } => {
            if *default == old_target {
                *default = new_target;
            }
            for (_, dest) in cases.iter_mut() {
                if *dest == old_target {
                    *dest = new_target;
                }
            }
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use llvm_ir::{Builder, Context, InstrId, IntPredicate, Linkage, Module, ValueRef};

    // -----------------------------------------------------------------------
    // Helper: count non-terminator instructions in a specific block
    // -----------------------------------------------------------------------
    fn body_len(func: &llvm_ir::Function, bid: BlockId) -> usize {
        func.blocks[bid.0 as usize].body.len()
    }

    // -----------------------------------------------------------------------
    // Helper: build the standard simple loop used by several tests.
    //
    //   entry  →  header  →  body  →  (back to header)
    //                      ↘  exit
    //
    //   body contains:
    //     %inv   = add %a, %bv      ← loop-invariant
    //     %i_next = add %i, 1       ← loop-variant
    // -----------------------------------------------------------------------
    fn build_invariant_add_loop() -> (Context, Module) {
        let mut ctx = Context::new();
        let mut module = Module::new("test");
        let mut b = Builder::new(&mut ctx, &mut module);
        b.add_function(
            "f",
            b.ctx.i32_ty,
            vec![b.ctx.i32_ty, b.ctx.i32_ty, b.ctx.i32_ty],
            vec!["n".into(), "a".into(), "bv".into()],
            false,
            Linkage::External,
        );

        let entry = b.add_block("entry");
        let header = b.add_block("header");
        let body = b.add_block("body");
        let exit_bb = b.add_block("exit");

        let c0 = b.const_int(b.ctx.i32_ty, 0);
        let c1 = b.const_int(b.ctx.i32_ty, 1);
        let n = b.get_arg(0);
        let a = b.get_arg(1);
        let bv = b.get_arg(2);

        b.position_at_end(entry);
        b.build_br(header);

        b.position_at_end(header);
        // Phi: %i = phi [0, entry], [%i_next, body]
        // We create the phi first with only the entry edge; we add the back-edge below.
        let i_phi = b.build_phi("i", b.ctx.i32_ty, vec![(c0, entry)]);
        let cmp = b.build_icmp("cmp", IntPredicate::Slt, i_phi, n);
        b.build_cond_br(cmp, body, exit_bb);

        b.position_at_end(body);
        let _inv = b.build_add("inv", a, bv); // loop-invariant: uses only args
        let _i_next = b.build_add("i_next", i_phi, c1); // loop-variant: uses loop phi
        b.build_br(header);

        b.position_at_end(exit_bb);
        b.build_ret(c0);
        drop(b);

        // Add back-edge to phi node.
        {
            let i_phi_iid = module.functions[0].value_names["i"];
            let i_next_iid = module.functions[0].value_names["i_next"];
            if let InstrKind::Phi { incoming, .. } =
                &mut module.functions[0].instructions[i_phi_iid.0 as usize].kind
            {
                incoming.push((ValueRef::Instruction(i_next_iid), body));
            }
        }

        (ctx, module)
    }

    // -----------------------------------------------------------------------
    // Test 1: Simple invariant add is hoisted out of the loop.
    // -----------------------------------------------------------------------
    #[test]
    fn licm_hoists_invariant_add() {
        let (mut ctx, mut module) = build_invariant_add_loop();

        // Before LICM: body contains %inv and %i_next (2 instrs).
        let func = &module.functions[0];
        let body = BlockId(2); // entry=0, header=1, body=2, exit=3
        assert_eq!(
            body_len(func, body),
            2,
            "before LICM: body should have 2 instrs"
        );

        let mut pass = Licm;
        let changed = pass.run_on_function(&mut ctx, &mut module.functions[0]);
        assert!(changed, "LICM must report a change");

        // After LICM: %inv must no longer be in body.
        let func = &module.functions[0];
        let inv_id = func.value_names["inv"];
        assert!(
            !func.blocks[body.0 as usize].body.contains(&inv_id),
            "%inv must be hoisted out of loop body"
        );

        // %i_next must still be in body (it is loop-variant).
        let i_next_id = func.value_names["i_next"];
        assert!(
            func.blocks[body.0 as usize].body.contains(&i_next_id),
            "%i_next must stay in loop body"
        );
    }

    // -----------------------------------------------------------------------
    // Test 2: Loop-variant instruction is NOT hoisted.
    // -----------------------------------------------------------------------
    #[test]
    fn licm_does_not_hoist_variant() {
        let (mut ctx, mut module) = build_invariant_add_loop();

        let mut pass = Licm;
        pass.run_on_function(&mut ctx, &mut module.functions[0]);

        // %i_next uses the loop phi, so it is variant and must stay in body.
        let func = &module.functions[0];
        let body = BlockId(2);
        let i_next_id = func.value_names["i_next"];
        assert!(
            func.blocks[body.0 as usize].body.contains(&i_next_id),
            "loop-variant %i_next must not be hoisted"
        );
    }

    // -----------------------------------------------------------------------
    // Test 3: Store is NOT hoisted (side effect).
    // -----------------------------------------------------------------------
    #[test]
    fn licm_does_not_hoist_store() {
        let mut ctx = Context::new();
        let mut module = Module::new("test");
        let mut b = Builder::new(&mut ctx, &mut module);
        b.add_function(
            "f",
            b.ctx.void_ty,
            vec![b.ctx.i32_ty],
            vec!["n".into()],
            false,
            Linkage::External,
        );

        let entry = b.add_block("entry");
        let header = b.add_block("header");
        let body = b.add_block("body");
        let exit_bb = b.add_block("exit");

        b.position_at_end(entry);
        let alloca = b.build_alloca("slot", b.ctx.i32_ty);
        b.build_br(header);

        b.position_at_end(header);
        let c0 = b.const_int(b.ctx.i32_ty, 0);
        let n = b.get_arg(0);
        let i_phi = b.build_phi("i", b.ctx.i32_ty, vec![(c0, entry)]);
        let cmp = b.build_icmp("cmp", IntPredicate::Slt, i_phi, n);
        b.build_cond_br(cmp, body, exit_bb);

        b.position_at_end(body);
        let c1 = b.const_int(b.ctx.i32_ty, 1);
        b.build_store(c1, alloca); // side effect — must never be hoisted
        let _i_next = b.build_add("i_next", i_phi, c1);
        b.build_br(header);

        b.position_at_end(exit_bb);
        b.build_ret_void();
        drop(b);

        {
            let i_phi_iid = module.functions[0].value_names["i"];
            let i_next_iid = module.functions[0].value_names["i_next"];
            if let InstrKind::Phi { incoming, .. } =
                &mut module.functions[0].instructions[i_phi_iid.0 as usize].kind
            {
                incoming.push((ValueRef::Instruction(i_next_iid), body));
            }
        }

        // Find the store instruction id before LICM.
        let store_id: InstrId = {
            let func = &module.functions[0];
            func.blocks[body.0 as usize]
                .body
                .iter()
                .find(|&&iid| matches!(func.instr(iid).kind, InstrKind::Store { .. }))
                .copied()
                .expect("store must exist in body before LICM")
        };

        let mut pass = Licm;
        pass.run_on_function(&mut ctx, &mut module.functions[0]);

        let func = &module.functions[0];
        assert!(
            func.blocks[body.0 as usize].body.contains(&store_id),
            "Store must not be hoisted out of loop body"
        );
    }

    // -----------------------------------------------------------------------
    // Test 4: Call is NOT hoisted (side effect).
    // -----------------------------------------------------------------------
    #[test]
    fn licm_does_not_hoist_call() {
        let mut ctx = Context::new();
        let mut module = Module::new("test");
        let mut b = Builder::new(&mut ctx, &mut module);

        // Declare an external function first (it will be at GlobalId(0) /
        // FunctionId(0)).
        let ext_fn_ty = b.ctx.mk_fn_type(b.ctx.i32_ty, vec![b.ctx.i32_ty], false);
        b.add_declaration("ext_fn", b.ctx.i32_ty, vec![b.ctx.i32_ty], false);

        // Main function under test will be at FunctionId(1).
        b.add_function(
            "f",
            b.ctx.i32_ty,
            vec![b.ctx.i32_ty],
            vec!["n".into()],
            false,
            Linkage::External,
        );

        let entry = b.add_block("entry");
        let header = b.add_block("header");
        let body = b.add_block("body");
        let exit_bb = b.add_block("exit");

        b.position_at_end(entry);
        b.build_br(header);

        b.position_at_end(header);
        let c0 = b.const_int(b.ctx.i32_ty, 0);
        let n = b.get_arg(0);
        let i_phi = b.build_phi("i", b.ctx.i32_ty, vec![(c0, entry)]);
        let cmp = b.build_icmp("cmp", IntPredicate::Slt, i_phi, n);
        b.build_cond_br(cmp, body, exit_bb);

        b.position_at_end(body);
        let c1 = b.const_int(b.ctx.i32_ty, 1);
        // call ext_fn(1) — has side effects, must not be hoisted
        let call_vr = b.build_call(
            "call_res",
            b.ctx.i32_ty,
            ext_fn_ty,
            ValueRef::Global(llvm_ir::GlobalId(0)),
            vec![c1],
        );
        let _i_next = b.build_add("i_next", i_phi, c1);
        b.build_br(header);

        b.position_at_end(exit_bb);
        b.build_ret(c0);
        drop(b);

        {
            let i_phi_iid = module.functions[1].value_names["i"];
            let i_next_iid = module.functions[1].value_names["i_next"];
            if let InstrKind::Phi { incoming, .. } =
                &mut module.functions[1].instructions[i_phi_iid.0 as usize].kind
            {
                incoming.push((ValueRef::Instruction(i_next_iid), body));
            }
        }

        let call_id = match call_vr {
            ValueRef::Instruction(id) => id,
            _ => panic!("build_call returned non-instruction"),
        };

        let mut pass = Licm;
        pass.run_on_function(&mut ctx, &mut module.functions[1]);

        let func = &module.functions[1];
        assert!(
            func.blocks[body.0 as usize].body.contains(&call_id),
            "Call must not be hoisted out of loop body"
        );
    }

    // -----------------------------------------------------------------------
    // Test 5: Total instruction count is unchanged (LICM moves, not copies).
    // -----------------------------------------------------------------------
    #[test]
    fn licm_moves_not_copies_instructions() {
        let (mut ctx, mut module) = build_invariant_add_loop();

        let total_before: usize = module.functions[0]
            .blocks
            .iter()
            .map(|bb| bb.body.len())
            .sum();

        let mut pass = Licm;
        let changed = pass.run_on_function(&mut ctx, &mut module.functions[0]);
        assert!(changed);

        let total_after: usize = module.functions[0]
            .blocks
            .iter()
            .map(|bb| bb.body.len())
            .sum();

        assert_eq!(
            total_before, total_after,
            "LICM should move (not copy) instructions — total body count unchanged"
        );
    }

    // -----------------------------------------------------------------------
    // Test 6: Nested loops — each invariant hoisted to its respective
    //         preheader.
    //
    //   entry → outer_header → outer_body → inner_header → inner_body
    //                        ↘ outer_exit               ↘ outer_exit
    //   outer_body contains %outer_inv = add %a, %bv    (outer-loop invariant)
    //   inner_body contains %inner_inv = add %cv, %dv   (inner-loop invariant)
    //                        %ii_next  = add %ii, 1     (inner-loop variant)
    // -----------------------------------------------------------------------
    #[test]
    fn licm_nested_loops_hoist_to_correct_preheader() {
        let mut ctx = Context::new();
        let mut module = Module::new("test");
        let mut b = Builder::new(&mut ctx, &mut module);
        b.add_function(
            "f",
            b.ctx.i32_ty,
            vec![
                b.ctx.i32_ty, // n
                b.ctx.i32_ty, // a
                b.ctx.i32_ty, // bv
                b.ctx.i32_ty, // cv
                b.ctx.i32_ty, // dv
            ],
            vec!["n".into(), "a".into(), "bv".into(), "cv".into(), "dv".into()],
            false,
            Linkage::External,
        );

        let entry = b.add_block("entry");
        let outer_header = b.add_block("outer_header");
        let outer_body = b.add_block("outer_body");
        let inner_header = b.add_block("inner_header");
        let inner_body = b.add_block("inner_body");
        let outer_exit = b.add_block("outer_exit");
        let exit_bb = b.add_block("exit");

        let c0 = b.const_int(b.ctx.i32_ty, 0);
        let c1 = b.const_int(b.ctx.i32_ty, 1);
        let n = b.get_arg(0);
        let a = b.get_arg(1);
        let bv = b.get_arg(2);
        let cv = b.get_arg(3);
        let dv = b.get_arg(4);

        // entry → outer_header
        b.position_at_end(entry);
        b.build_br(outer_header);

        // outer_header: %oi = phi; cond branch
        b.position_at_end(outer_header);
        let oi = b.build_phi("oi", b.ctx.i32_ty, vec![(c0, entry)]);
        let ocmp = b.build_icmp("ocmp", IntPredicate::Slt, oi, n);
        b.build_cond_br(ocmp, outer_body, exit_bb);

        // outer_body: invariant add, then fall to inner_header
        b.position_at_end(outer_body);
        let _outer_inv = b.build_add("outer_inv", a, bv); // outer-loop invariant
        b.build_br(inner_header);

        // inner_header: %ii = phi; cond branch
        b.position_at_end(inner_header);
        let ii = b.build_phi("ii", b.ctx.i32_ty, vec![(c0, outer_body)]);
        let icmp = b.build_icmp("icmp", IntPredicate::Slt, ii, n);
        b.build_cond_br(icmp, inner_body, outer_exit);

        // inner_body: inner invariant + counter increment
        b.position_at_end(inner_body);
        let _inner_inv = b.build_add("inner_inv", cv, dv); // inner-loop invariant
        let _ii_next = b.build_add("ii_next", ii, c1); // inner-loop variant
        b.build_br(inner_header);

        // outer_exit: increment outer counter, loop back
        b.position_at_end(outer_exit);
        let _oi_next = b.build_add("oi_next", oi, c1);
        b.build_br(outer_header);

        b.position_at_end(exit_bb);
        b.build_ret(c0);
        drop(b);

        // Add back-edge to inner phi.
        {
            let ii_phi_iid = module.functions[0].value_names["ii"];
            let ii_next_iid = module.functions[0].value_names["ii_next"];
            if let InstrKind::Phi { incoming, .. } =
                &mut module.functions[0].instructions[ii_phi_iid.0 as usize].kind
            {
                incoming.push((ValueRef::Instruction(ii_next_iid), inner_body));
            }
        }

        // Add back-edge to outer phi.
        {
            let oi_phi_iid = module.functions[0].value_names["oi"];
            let oi_next_iid = module.functions[0].value_names["oi_next"];
            if let InstrKind::Phi { incoming, .. } =
                &mut module.functions[0].instructions[oi_phi_iid.0 as usize].kind
            {
                incoming.push((ValueRef::Instruction(oi_next_iid), outer_exit));
            }
        }

        // Before LICM.
        let func = &module.functions[0];
        assert_eq!(body_len(func, outer_body), 1, "outer_body: 1 instr before LICM");
        assert_eq!(body_len(func, inner_body), 2, "inner_body: 2 instrs before LICM");

        let mut pass = Licm;
        let changed = pass.run_on_function(&mut ctx, &mut module.functions[0]);
        assert!(changed, "LICM should make changes on nested loops");

        // After LICM: both invariant adds must have left their respective loops.
        let func = &module.functions[0];
        let inner_inv_id = func.value_names["inner_inv"];
        assert!(
            !func.blocks[inner_body.0 as usize].body.contains(&inner_inv_id),
            "%inner_inv must be hoisted out of inner_body"
        );

        let outer_inv_id = func.value_names["outer_inv"];
        assert!(
            !func.blocks[outer_body.0 as usize].body.contains(&outer_inv_id),
            "%outer_inv must be hoisted out of outer_body"
        );
    }
}
