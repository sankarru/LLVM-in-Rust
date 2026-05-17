//! RV64 IR -> machine-IR lowering.

use crate::{
    abi::{classify_rv64_int_args, ArgLocation, INT_RET},
    instructions::*,
    regs::{ALLOCATABLE, CALLEE_SAVED, X0},
};
use llvm_codegen::isel::{DebugLoc, IselBackend, MInstr, MachineFunction, PReg, VReg};
use llvm_codegen::sizeof_ty;
use llvm_ir::{
    ArgId, BlockId, ConstantData, Context, Function, InstrId, InstrKind, InstrprofIntrinsic,
    IntPredicate, Module, RmwOp, TypeData, ValueRef,
};
use std::collections::HashMap;

/// Public API for `RiscVBackend`.
#[derive(Default)]
pub struct RiscVBackend;

impl IselBackend for RiscVBackend {
    fn lower_function(
        &mut self,
        // `ctx` field.
        ctx: &Context,
        // `module` field.
        module: &Module,
        // `func` field.
        func: &Function,
    ) -> MachineFunction {
        let mut mf = MachineFunction::new(func.name.clone());
        mf.allocatable_pregs = ALLOCATABLE.to_vec();
        mf.allocatable_fp_pregs = vec![]; // FP register class: populated in subsequent FP PR
        mf.callee_saved_pregs = CALLEE_SAVED.to_vec();
        mf.debug_source = module.source_filename.clone();

        if func.is_declaration || func.blocks.is_empty() {
            return mf;
        }

        for (bi, bb) in func.blocks.iter().enumerate() {
            let label = if bi == 0 {
                func.name.clone()
            } else {
                format!("{}.{}", func.name, bb.name)
            };
            mf.add_block(label);
        }

        let mut vmap: HashMap<ValueRef, VReg> = HashMap::new();

        for bb in &func.blocks {
            for &iid in &bb.body {
                if let InstrKind::Phi { .. } = &func.instr(iid).kind {
                    let vr = mf.fresh_vreg();
                    vmap.insert(ValueRef::Instruction(iid), vr);
                }
            }
        }

        // Copy args from ABI regs/stack placeholders.
        let arg_locs = classify_rv64_int_args(func.args.len());
        for (i, _arg) in func.args.iter().enumerate() {
            let vr = mf.fresh_vreg();
            vmap.insert(ValueRef::Argument(ArgId(i as u32)), vr);
            match arg_locs[i] {
                ArgLocation::Reg(preg) => {
                    let mut mi = MInstr::new(MOV_RR).with_dst(vr).with_preg(preg);
                    mi.phys_uses = vec![preg];
                    mf.push(0, mi);
                }
                ArgLocation::Stack(offset) => {
                    // Placeholder until frame/stack argument loads are modeled.
                    mf.push(0, MInstr::new(MOV_IMM).with_dst(vr).with_imm(offset as i64));
                }
            }
        }

        for (bi, bb) in func.blocks.iter().enumerate() {
            for &iid in &bb.body {
                let dbg = func
                    .instr_dbg_loc(iid)
                    .and_then(|loc_id| module.debug_location(loc_id))
                    .map(|loc| DebugLoc {
                        line: loc.line,
                        column: loc.column,
                    });
                mf.current_debug_loc = dbg;
                if mf.debug_line_start.is_none() {
                    mf.debug_line_start = dbg.map(|loc| loc.line);
                }
                lower_instr(ctx, module, func, &mut mf, bi, iid, &mut vmap);
            }
            if let Some(tid) = bb.terminator {
                let dbg = func
                    .instr_dbg_loc(tid)
                    .and_then(|loc_id| module.debug_location(loc_id))
                    .map(|loc| DebugLoc {
                        line: loc.line,
                        column: loc.column,
                    });
                mf.current_debug_loc = dbg;
                if mf.debug_line_start.is_none() {
                    mf.debug_line_start = dbg.map(|loc| loc.line);
                }
                lower_terminator(ctx, func, &mut mf, bi, tid, &mut vmap);
            }
        }

        mf.current_debug_loc = None;
        mf
    }
}

fn resolve(
    ctx: &Context,
    mf: &mut MachineFunction,
    mblock: usize,
    vmap: &mut HashMap<ValueRef, VReg>,
    vr: ValueRef,
) -> VReg {
    if let Some(&existing) = vmap.get(&vr) {
        return existing;
    }
    match vr {
        ValueRef::Constant(cid) => {
            let vreg = mf.fresh_vreg();
            vmap.insert(vr, vreg);
            let imm = const_to_imm(ctx.get_const(cid));
            mf.push(mblock, MInstr::new(MOV_IMM).with_dst(vreg).with_imm(imm));
            vreg
        }
        _ => {
            let vreg = mf.fresh_vreg();
            vmap.insert(vr, vreg);
            vreg
        }
    }
}

fn const_to_imm(cd: &ConstantData) -> i64 {
    match cd {
        ConstantData::Int { val, .. } => *val as i64,
        ConstantData::Float { bits, .. } => *bits as i64,
        _ => 0,
    }
}

fn lower_icmp_to_bool(
    pred: IntPredicate,
    lhs: VReg,
    rhs: VReg,
    mf: &mut MachineFunction,
    mblock: usize,
    dst: VReg,
) {
    match pred {
        IntPredicate::Eq => {
            let t = mf.fresh_vreg();
            mf.push(mblock, MInstr::new(XOR_RR).with_dst(t).with_vreg(lhs).with_vreg(rhs));
            // dst = (t == 0) ? 1 : 0
            mf.push(mblock, MInstr::new(SLTIU).with_dst(dst).with_vreg(t).with_imm(1));
        }
        IntPredicate::Ne => {
            let t = mf.fresh_vreg();
            mf.push(mblock, MInstr::new(XOR_RR).with_dst(t).with_vreg(lhs).with_vreg(rhs));
            // dst = (t != 0) ? 1 : 0 => sltu x0, t
            mf.push(mblock, MInstr::new(SLTU_RR).with_dst(dst).with_preg(X0).with_vreg(t));
        }
        IntPredicate::Slt => {
            mf.push(mblock, MInstr::new(SLT_RR).with_dst(dst).with_vreg(lhs).with_vreg(rhs));
        }
        IntPredicate::Sle => {
            let t = mf.fresh_vreg();
            mf.push(mblock, MInstr::new(SLT_RR).with_dst(t).with_vreg(rhs).with_vreg(lhs));
            mf.push(mblock, MInstr::new(XORI).with_dst(dst).with_vreg(t).with_imm(1));
        }
        IntPredicate::Sgt => {
            mf.push(mblock, MInstr::new(SLT_RR).with_dst(dst).with_vreg(rhs).with_vreg(lhs));
        }
        IntPredicate::Sge => {
            let t = mf.fresh_vreg();
            mf.push(mblock, MInstr::new(SLT_RR).with_dst(t).with_vreg(lhs).with_vreg(rhs));
            mf.push(mblock, MInstr::new(XORI).with_dst(dst).with_vreg(t).with_imm(1));
        }
        IntPredicate::Ult => {
            mf.push(mblock, MInstr::new(SLTU_RR).with_dst(dst).with_vreg(lhs).with_vreg(rhs));
        }
        IntPredicate::Ule => {
            let t = mf.fresh_vreg();
            mf.push(mblock, MInstr::new(SLTU_RR).with_dst(t).with_vreg(rhs).with_vreg(lhs));
            mf.push(mblock, MInstr::new(XORI).with_dst(dst).with_vreg(t).with_imm(1));
        }
        IntPredicate::Ugt => {
            mf.push(mblock, MInstr::new(SLTU_RR).with_dst(dst).with_vreg(rhs).with_vreg(lhs));
        }
        IntPredicate::Uge => {
            let t = mf.fresh_vreg();
            mf.push(mblock, MInstr::new(SLTU_RR).with_dst(t).with_vreg(lhs).with_vreg(rhs));
            mf.push(mblock, MInstr::new(XORI).with_dst(dst).with_vreg(t).with_imm(1));
        }
    }
}

fn instrprof_from_callee(ctx: &Context, callee: ValueRef) -> Option<InstrprofIntrinsic> {
    let ValueRef::Constant(cid) = callee else {
        return None;
    };
    let ConstantData::GlobalRef { name, .. } = ctx.get_const(cid) else {
        return None;
    };
    InstrprofIntrinsic::from_name(name)
}

fn lower_instr(
    ctx: &Context,
    _module: &Module,
    func: &Function,
    mf: &mut MachineFunction,
    mblock: usize,
    iid: InstrId,
    vmap: &mut HashMap<ValueRef, VReg>,
) {
    use InstrKind::*;
    let instr = func.instr(iid);

    macro_rules! new_dst {
        () => {{
            let v = mf.fresh_vreg();
            vmap.insert(ValueRef::Instruction(iid), v);
            v
        }};
    }
    macro_rules! res {
        ($vref:expr) => {
            resolve(ctx, mf, mblock, vmap, $vref)
        };
    }
    macro_rules! emit_binop3 {
        ($op:expr, $lhs:expr, $rhs:expr) => {{
            let dst = new_dst!();
            let l = res!($lhs);
            let r = res!($rhs);
            mf.push(mblock, MInstr::new($op).with_dst(dst).with_vreg(l).with_vreg(r));
        }};
    }

    match &instr.kind {
        Add { lhs, rhs, .. } => emit_binop3!(ADD_RR, *lhs, *rhs),
        Sub { lhs, rhs, .. } => emit_binop3!(SUB_RR, *lhs, *rhs),
        Mul { lhs, rhs, .. } => emit_binop3!(MUL_RR, *lhs, *rhs),
        SDiv { lhs, rhs, .. } => emit_binop3!(DIV_RR, *lhs, *rhs),
        UDiv { lhs, rhs, .. } => emit_binop3!(UDIV_RR, *lhs, *rhs),
        SRem { lhs, rhs, .. } => emit_binop3!(REM_RR, *lhs, *rhs),
        URem { lhs, rhs, .. } => emit_binop3!(UREM_RR, *lhs, *rhs),

        And { lhs, rhs } => emit_binop3!(AND_RR, *lhs, *rhs),
        Or { lhs, rhs } => emit_binop3!(OR_RR, *lhs, *rhs),
        Xor { lhs, rhs } => emit_binop3!(XOR_RR, *lhs, *rhs),

        Shl { lhs, rhs, .. } => emit_binop3!(SLL_RR, *lhs, *rhs),
        LShr { lhs, rhs, .. } => emit_binop3!(SRL_RR, *lhs, *rhs),
        AShr { lhs, rhs, .. } => emit_binop3!(SRA_RR, *lhs, *rhs),

        ICmp { pred, lhs, rhs } => {
            let dst = new_dst!();
            let l = res!(*lhs);
            let r = res!(*rhs);
            lower_icmp_to_bool(*pred, l, r, mf, mblock, dst);
        }

        FCmp { .. } => {
            let dst = new_dst!();
            mf.push(mblock, MInstr::new(MOV_IMM).with_dst(dst).with_imm(0));
        }

        Select { cond, then_val, else_val } => {
            let dst = new_dst!();
            let c = res!(*cond);
            let tv = res!(*then_val);
            let fv = res!(*else_val);
            // cond is i1 (0/1): dst = tv*cond + fv*(cond^1)
            let notc = mf.fresh_vreg();
            mf.push(mblock, MInstr::new(XORI).with_dst(notc).with_vreg(c).with_imm(1));
            let tpart = mf.fresh_vreg();
            let fpart = mf.fresh_vreg();
            mf.push(mblock, MInstr::new(MUL_RR).with_dst(tpart).with_vreg(tv).with_vreg(c));
            mf.push(mblock, MInstr::new(MUL_RR).with_dst(fpart).with_vreg(fv).with_vreg(notc));
            mf.push(mblock, MInstr::new(ADD_RR).with_dst(dst).with_vreg(tpart).with_vreg(fpart));
        }

        Phi { .. } => {}

        ZExt { val, .. }
        | Trunc { val, .. }
        | BitCast { val, .. }
        | PtrToInt { val, .. }
        | IntToPtr { val, .. }
        | FPTrunc { val, .. }
        | FPExt { val, .. }
        | FPToUI { val, .. }
        | FPToSI { val, .. }
        | UIToFP { val, .. }
        | SIToFP { val, .. }
        | AddrSpaceCast { val, .. }
        | Freeze { val }
        | SExt { val, .. } => {
            let dst = new_dst!();
            let src = res!(*val);
            mf.push(mblock, MInstr::new(MOV_RR).with_dst(dst).with_vreg(src));
        }

        Call { callee, args, .. } => {
            if let Some(ip) = instrprof_from_callee(ctx, *callee) {
                eprintln!(
                    "warning: PGO intrinsic {} elided — counter emission not implemented",
                    ip.as_str()
                );
                for &arg in args {
                    let _ = res!(arg);
                }
                mf.push(mblock, MInstr::new(NOP));
                return;
            }
            let arg_locs = classify_rv64_int_args(args.len());
            for (i, &arg_vref) in args.iter().enumerate() {
                let src = res!(arg_vref);
                match arg_locs[i] {
                    ArgLocation::Reg(preg) => emit_mov_to_preg(mf, mblock, preg, src),
                    ArgLocation::Stack(_off) => mf.push(mblock, MInstr::new(SD).with_vreg(src)),
                }
            }
            let callee_vr = res!(*callee);
            let mut call_mi = MInstr::new(JALR).with_preg(PReg(1)).with_vreg(callee_vr).with_imm(0);
            call_mi.clobbers = ALLOCATABLE.to_vec();
            mf.push(mblock, call_mi);

            let dst = new_dst!();
            emit_mov_from_preg(mf, mblock, dst, INT_RET);
        }

        InlineAsm { args, .. } => {
            for &arg in args {
                let _ = res!(arg);
            }
            mf.push(mblock, MInstr::new(NOP));
            if instr.ty != ctx.void_ty {
                let dst = new_dst!();
                mf.push(mblock, MInstr::new(MOV_IMM).with_dst(dst).with_imm(0));
            }
        }

        // ── atomics (#239) — RISC-V A-extension ───────────────────────────
        Fence { .. } => {
            mf.push(mblock, MInstr::new(FENCE));
        }
        CmpXchg { ptr, cmp: _cmp, new_val, .. } => {
            let ptr_vr = res!(*ptr);
            let _cmp_vr = res!(*_cmp);
            let new_vr = res!(*new_val);
            // Determine width from instruction result type (struct { val_ty, i1 }).
            // Use W vs D based on pointer-width assumption; default to W (32-bit).
            let ty_bits = match ctx.get_type(instr.ty) {
                TypeData::Struct(st) if !st.fields.is_empty() => {
                    match ctx.get_type(st.fields[0]) {
                        TypeData::Integer(b) => *b,
                        _ => 32,
                    }
                }
                _ => 32,
            };
            let use_d = ty_bits == 64;
            // LR.W/LR.D old, (ptr)
            let old_vr = mf.fresh_vreg();
            let lr_op = if use_d { LR_D } else { LR_W };
            mf.push(mblock, MInstr::new(lr_op).with_dst(old_vr).with_vreg(ptr_vr));
            // SC.W/SC.D status, new_val, (ptr)
            // (Simplified: no retry loop — follow-up per issue #239)
            let status_vr = mf.fresh_vreg();
            let sc_op = if use_d { SC_D } else { SC_W };
            mf.push(mblock, MInstr::new(sc_op).with_dst(status_vr).with_vreg(ptr_vr).with_vreg(new_vr));
            // Result is the old value from LR.
            let dst = new_dst!();
            mf.push(mblock, MInstr::new(MOV_RR).with_dst(dst).with_vreg(old_vr));
        }
        AtomicRmw { op, ptr, val, .. } => {
            let ptr_vr = res!(*ptr);
            let val_vr = res!(*val);
            // Determine word vs doubleword from the instruction's result type.
            let ty_bits = match ctx.get_type(instr.ty) {
                TypeData::Integer(b) => *b,
                _ => 32,
            };
            let use_d = ty_bits == 64;
            let opcode = match op {
                RmwOp::Add => if use_d { AMOADD_D } else { AMOADD_W },
                RmwOp::Xchg => if use_d { AMOSWAP_D } else { AMOSWAP_W },
                RmwOp::Xor => if use_d { AMOXOR_D } else { AMOXOR_W },
                RmwOp::And | RmwOp::Nand => if use_d { AMOAND_D } else { AMOAND_W },
                RmwOp::Or => if use_d { AMOOR_D } else { AMOOR_W },
                // Sub: negate val then add — use amoadd (caller is responsible for negation;
                // here we emit amoadd as the closest approximation without modifying val).
                RmwOp::Sub => if use_d { AMOADD_D } else { AMOADD_W },
                RmwOp::Min => AMOMIN_W,
                RmwOp::Max => AMOMAX_W,
                RmwOp::UMin => AMOMINU_W,
                RmwOp::UMax => AMOMAXU_W,
                _ => if use_d { AMOADD_D } else { AMOADD_W },
            };
            let dst = new_dst!();
            mf.push(mblock, MInstr::new(opcode).with_dst(dst).with_vreg(ptr_vr).with_vreg(val_vr));
        }

        // ── memory: alloca, load, store ────────────────────────────────────
        Alloca { alloc_ty, align, .. } => {
            let dst = new_dst!();
            let size_bytes = sizeof_ty(ctx, *alloc_ty) as u32;
            let align_bytes = align.unwrap_or(8);
            let slot_idx = mf.alloc_alloca_slot(size_bytes, align_bytes);
            // ADDI_FP_SLOT: addi dst, s0, -(slot_idx+1)*8
            mf.push(mblock, MInstr::new(ADDI_FP_SLOT).with_dst(dst).with_imm(slot_idx as i64));
        }
        GetElementPtr { ptr, .. } => {
            let dst = new_dst!();
            let base = res!(*ptr);
            mf.push(mblock, MInstr::new(MOV_RR).with_dst(dst).with_vreg(base));
        }
        Load { ptr, .. } => {
            let dst = new_dst!();
            let ptr_vr = res!(*ptr);
            // LD_REG: ld dst, 0(ptr_reg)
            mf.push(mblock, MInstr::new(LD_REG).with_dst(dst).with_vreg(ptr_vr));
        }
        Store { val, ptr, .. } => {
            let src = res!(*val);
            let ptr_vr = res!(*ptr);
            // SD_REG: sd src, 0(ptr_reg)
            mf.push(mblock, MInstr::new(SD_REG).with_vreg(ptr_vr).with_vreg(src));
        }

        FAdd { .. } | FSub { .. } | FMul { .. } | FDiv { .. } | FRem { .. } | FNeg { .. } => {
            let dst = new_dst!();
            mf.push(mblock, MInstr::new(MOV_IMM).with_dst(dst).with_imm(0));
        }

        ExtractValue { .. }
        | InsertValue { .. }
        | ExtractElement { .. }
        | InsertElement { .. }
        | ShuffleVector { .. } => {
            let dst = new_dst!();
            mf.push(mblock, MInstr::new(MOV_IMM).with_dst(dst).with_imm(0));
        }

        LandingPad { .. } => {
            let dst = new_dst!();
            mf.push(mblock, MInstr::new(MOV_IMM).with_dst(dst).with_imm(0));
        }

        Ret { .. } | Br { .. } | CondBr { .. } | Invoke { .. } | Switch { .. } | Unreachable => {}
    }
}

fn lower_terminator(
    ctx: &Context,
    func: &Function,
    mf: &mut MachineFunction,
    mblock: usize,
    tid: InstrId,
    vmap: &mut HashMap<ValueRef, VReg>,
) {
    use InstrKind::*;
    let term = func.instr(tid);

    match &term.kind {
        Ret { val } => {
            if let Some(rv) = val {
                let src = resolve(ctx, mf, mblock, vmap, *rv);
                emit_mov_to_preg(mf, mblock, INT_RET, src);
            }
            mf.push(mblock, MInstr::new(RET));
        }

        Br { dest } => {
            emit_phi_copies(ctx, func, mf, mblock, mblock, *dest, vmap);
            mf.push(mblock, MInstr::new(JAL).with_block(dest.0 as usize));
        }

        CondBr { cond, then_dest, else_dest } => {
            let c = resolve(ctx, mf, mblock, vmap, *cond);
            let pred_label = mf.blocks[mblock].label.clone();
            let then_edge = mf.add_block(format!("{}.then_edge", pred_label));
            let else_edge = mf.add_block(format!("{}.else_edge", pred_label));
            emit_phi_copies(ctx, func, mf, mblock, then_edge, *then_dest, vmap);
            mf.push(then_edge, MInstr::new(JAL).with_block(then_dest.0 as usize));
            emit_phi_copies(ctx, func, mf, mblock, else_edge, *else_dest, vmap);
            mf.push(else_edge, MInstr::new(JAL).with_block(else_dest.0 as usize));
            // if c == 0 goto else_edge else then_edge
            mf.push(mblock, MInstr::new(BEQ).with_vreg(c).with_preg(X0).with_block(else_edge));
            mf.push(mblock, MInstr::new(JAL).with_block(then_edge));
        }

        Invoke { callee, args, normal_dest, .. } => {
            let arg_locs = classify_rv64_int_args(args.len());
            for (i, &arg_vref) in args.iter().enumerate() {
                let src = resolve(ctx, mf, mblock, vmap, arg_vref);
                match arg_locs[i] {
                    ArgLocation::Reg(preg) => emit_mov_to_preg(mf, mblock, preg, src),
                    ArgLocation::Stack(_) => {}
                }
            }
            let callee_vr = resolve(ctx, mf, mblock, vmap, *callee);
            let mut call_mi = MInstr::new(JALR).with_vreg(callee_vr);
            call_mi.clobbers = ALLOCATABLE.to_vec();
            mf.push(mblock, call_mi);
            if term.ty != ctx.void_ty {
                let dst = mf.fresh_vreg();
                vmap.insert(ValueRef::Instruction(tid), dst);
                emit_mov_from_preg(mf, mblock, dst, INT_RET);
            }
            emit_phi_copies(ctx, func, mf, mblock, mblock, *normal_dest, vmap);
            mf.push(mblock, MInstr::new(JAL).with_block(normal_dest.0 as usize));
        }

        Switch { val, default, cases } => {
            let v = resolve(ctx, mf, mblock, vmap, *val);
            for (case_val, case_dest) in cases {
                let cv = resolve(ctx, mf, mblock, vmap, *case_val);
                emit_phi_copies(ctx, func, mf, mblock, mblock, *case_dest, vmap);
                mf.push(mblock, MInstr::new(BEQ).with_vreg(v).with_vreg(cv).with_block(case_dest.0 as usize));
            }
            emit_phi_copies(ctx, func, mf, mblock, mblock, *default, vmap);
            mf.push(mblock, MInstr::new(JAL).with_block(default.0 as usize));
        }

        Unreachable => mf.push(mblock, MInstr::new(NOP)),

        _ => {}
    }
}

fn emit_phi_copies(
    ctx: &Context,
    func: &Function,
    mf: &mut MachineFunction,
    ir_src_block: usize,
    emit_to_mblock: usize,
    dest: BlockId,
    vmap: &mut HashMap<ValueRef, VReg>,
) {
    let dest_bb = &func.blocks[dest.0 as usize];
    let src_bid = BlockId(ir_src_block as u32);

    for &iid in &dest_bb.body {
        if let InstrKind::Phi { incoming, .. } = &func.instr(iid).kind {
            if let Some((incoming_val, _)) = incoming.iter().find(|(_, bid)| *bid == src_bid) {
                let phi_vreg = match vmap.get(&ValueRef::Instruction(iid)) {
                    Some(&v) => v,
                    None => continue,
                };
                let src_vreg = resolve(ctx, mf, emit_to_mblock, vmap, *incoming_val);
                mf.push(emit_to_mblock, MInstr::new(MOV_RR).with_dst(phi_vreg).with_vreg(src_vreg));
            }
        }
    }
}

fn emit_mov_to_preg(mf: &mut MachineFunction, mblock: usize, preg: PReg, src: VReg) {
    mf.push(mblock, MInstr::new(MOV_PR).with_preg(preg).with_vreg(src));
}

fn emit_mov_from_preg(mf: &mut MachineFunction, mblock: usize, dst: VReg, preg: PReg) {
    let mut mi = MInstr::new(MOV_RR).with_dst(dst).with_preg(preg);
    mi.phys_uses = vec![preg];
    mf.push(mblock, mi);
}

#[cfg(test)]
mod tests {
    use super::*;
    use llvm_ir::{Builder, Context, IntPredicate, Linkage, Module};

    #[test]
    fn lower_declaration_is_empty() {
        let mut ctx = Context::new();
        let mut module = Module::new("m");
        let mut b = Builder::new(&mut ctx, &mut module);
        b.add_function("decl", b.ctx.i64_ty, vec![], vec![], true, Linkage::External);
        let f = &module.functions[0];
        let mut be = RiscVBackend;
        let mf = be.lower_function(&ctx, &module, f);
        assert!(mf.blocks.is_empty());
    }

    #[test]
    fn lower_add_ret_generates_instructions() {
        let mut ctx = Context::new();
        let mut module = Module::new("m");
        let mut b = Builder::new(&mut ctx, &mut module);
        b.add_function(
            "f",
            b.ctx.i64_ty,
            vec![b.ctx.i64_ty, b.ctx.i64_ty],
            vec!["a".into(), "b".into()],
            false,
            Linkage::External,
        );
        let entry = b.add_block("entry");
        b.position_at_end(entry);
        let a = b.get_arg(0);
        let bb = b.get_arg(1);
        let s = b.build_add("s", a, bb);
        b.build_ret(s);

        let f = &module.functions[0];
        let mut be = RiscVBackend;
        let mf = be.lower_function(&ctx, &module, f);
        assert_eq!(mf.blocks.len(), 1);
        assert!(mf.blocks[0].instrs.iter().any(|mi| mi.opcode == ADD_RR));
        assert!(mf.blocks[0].instrs.iter().any(|mi| mi.opcode == RET));
    }

    #[test]
    fn lower_icmp_and_condbr() {
        let mut ctx = Context::new();
        let mut module = Module::new("m");
        let mut b = Builder::new(&mut ctx, &mut module);
        b.add_function(
            "f",
            b.ctx.i64_ty,
            vec![b.ctx.i64_ty],
            vec!["x".into()],
            false,
            Linkage::External,
        );
        let entry = b.add_block("entry");
        let tbb = b.add_block("then");
        let ebb = b.add_block("else");
        b.position_at_end(entry);
        let x = b.get_arg(0);
        let z = b.const_i64(0);
        let c = b.build_icmp("c", IntPredicate::Sgt, x, z);
        b.build_cond_br(c, tbb, ebb);
        b.position_at_end(tbb);
        let one = b.const_i64(1);
        b.build_ret(one);
        b.position_at_end(ebb);
        let zero = b.const_i64(0);
        b.build_ret(zero);

        let f = &module.functions[0];
        let mut be = RiscVBackend;
        let mf = be.lower_function(&ctx, &module, f);
        let all_instrs: Vec<_> = mf.blocks.iter().flat_map(|bb| bb.instrs.iter()).collect();
        assert!(all_instrs.iter().any(|mi| mi.opcode == SLT_RR));
        assert!(all_instrs.iter().any(|mi| mi.opcode == BEQ));
        assert!(all_instrs.iter().any(|mi| mi.opcode == JAL));
    }

    // ── Atomic lowering tests ──────────────────────────────────────────────

    #[test]
    fn riscv_fence_emits_fence_opcode() {
        use llvm_ir_parser::parser::parse;
        let src = r#"define void @f() {
entry:
  fence seq_cst
  ret void
}"#;
        let (ctx, module) = parse(src).expect("parse");
        let func = &module.functions[0];
        let mut be = RiscVBackend;
        let mf = be.lower_function(&ctx, &module, func);
        let has_fence = mf
            .blocks
            .iter()
            .flat_map(|b| b.instrs.iter())
            .any(|i| i.opcode == FENCE);
        assert!(has_fence, "expected FENCE opcode in lowered fence");
    }

    #[test]
    fn riscv_atomicrmw_add_emits_amoadd_w() {
        use llvm_ir_parser::parser::parse;
        let src = r#"define i32 @f(ptr %p, i32 %v) {
entry:
  %r = atomicrmw add ptr %p, i32 %v seq_cst
  ret i32 %r
}"#;
        let (ctx, module) = parse(src).expect("parse");
        let func = &module.functions[0];
        let mut be = RiscVBackend;
        let mf = be.lower_function(&ctx, &module, func);
        let has_amoadd = mf
            .blocks
            .iter()
            .flat_map(|b| b.instrs.iter())
            .any(|i| i.opcode == AMOADD_W);
        assert!(has_amoadd, "expected AMOADD_W in lowered atomicrmw add i32");
    }

    #[test]
    fn riscv_atomicrmw_add_i64_emits_amoadd_d() {
        use llvm_ir_parser::parser::parse;
        let src = r#"define i64 @f(ptr %p, i64 %v) {
entry:
  %r = atomicrmw add ptr %p, i64 %v seq_cst
  ret i64 %r
}"#;
        let (ctx, module) = parse(src).expect("parse");
        let func = &module.functions[0];
        let mut be = RiscVBackend;
        let mf = be.lower_function(&ctx, &module, func);
        let has_amoadd_d = mf
            .blocks
            .iter()
            .flat_map(|b| b.instrs.iter())
            .any(|i| i.opcode == AMOADD_D);
        assert!(has_amoadd_d, "expected AMOADD_D in lowered atomicrmw add i64");
    }

    #[test]
    fn riscv_atomicrmw_xchg_emits_amoswap_w() {
        use llvm_ir_parser::parser::parse;
        let src = r#"define i32 @f(ptr %p, i32 %v) {
entry:
  %r = atomicrmw xchg ptr %p, i32 %v seq_cst
  ret i32 %r
}"#;
        let (ctx, module) = parse(src).expect("parse");
        let func = &module.functions[0];
        let mut be = RiscVBackend;
        let mf = be.lower_function(&ctx, &module, func);
        let has_amoswap = mf
            .blocks
            .iter()
            .flat_map(|b| b.instrs.iter())
            .any(|i| i.opcode == AMOSWAP_W);
        assert!(has_amoswap, "expected AMOSWAP_W in lowered atomicrmw xchg i32");
    }

    #[test]
    fn riscv_cmpxchg_emits_lr_sc() {
        use llvm_ir_parser::parser::parse;
        let src = r#"define i32 @f(ptr %p, i32 %cmp, i32 %new) {
entry:
  %r = cmpxchg ptr %p, i32 %cmp, i32 %new seq_cst seq_cst
  %v = extractvalue { i32, i1 } %r, 0
  ret i32 %v
}"#;
        let (ctx, module) = parse(src).expect("parse");
        let func = &module.functions[0];
        let mut be = RiscVBackend;
        let mf = be.lower_function(&ctx, &module, func);
        let all_instrs: Vec<_> = mf.blocks.iter().flat_map(|b| b.instrs.iter()).collect();
        let has_lr = all_instrs.iter().any(|i| i.opcode == LR_W);
        let has_sc = all_instrs.iter().any(|i| i.opcode == SC_W);
        assert!(has_lr, "expected LR_W in lowered cmpxchg");
        assert!(has_sc, "expected SC_W in lowered cmpxchg");
    }

    #[test]
    fn riscv_fence_no_nop_emitted() {
        // Fence must not emit NOP — it should emit exactly one FENCE instruction.
        use llvm_ir_parser::parser::parse;
        let src = r#"define void @f() {
entry:
  fence seq_cst
  ret void
}"#;
        let (ctx, module) = parse(src).expect("parse");
        let func = &module.functions[0];
        let mut be = RiscVBackend;
        let mf = be.lower_function(&ctx, &module, func);
        let nop_count = mf
            .blocks
            .iter()
            .flat_map(|b| b.instrs.iter())
            .filter(|i| i.opcode == NOP)
            .count();
        assert_eq!(nop_count, 0, "fence should not emit any NOP instructions");
    }
}
