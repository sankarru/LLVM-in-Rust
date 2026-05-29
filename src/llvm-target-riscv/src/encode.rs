//! RV64 encoder and object emission.

use crate::{
    instructions::*,
    regs::{fp_enc, is_fp_reg},
};
use llvm_codegen::emit::{DebugLineRow, Emitter, ObjectFormat, Section};
use llvm_codegen::isel::{MInstr, MOpcode, MOperand, MachineFunction, PReg, VReg};

/// Public API for `RiscVEmitter`.
pub struct RiscVEmitter {
    /// Public API for `format`.
    pub format: ObjectFormat,
}

impl RiscVEmitter {
    /// Public API for `new`.
    pub fn new(format: ObjectFormat) -> Self {
        Self { format }
    }
}

impl Emitter for RiscVEmitter {
    fn emit_function(&mut self, mf: &MachineFunction) -> Section {
        let mut data = Vec::new();
        let mut patches: Vec<(usize, usize, PatchKind, MOpcode)> = Vec::new();
        let mut debug_rows: Vec<DebugLineRow> = Vec::new();

        for block in &mf.blocks {
            for instr in &block.instrs {
                let off = data.len();
                if let Some(loc) = instr.debug_loc {
                    debug_rows.push(DebugLineRow {
                        address: off as u64,
                        line: loc.line,
                        column: loc.column,
                    });
                }
                if let Some((target, kind)) = patch_from_instr(instr) {
                    patches.push((off, target, kind, instr.opcode));
                }
                let w = encode_instr(instr);
                data.extend_from_slice(&w.to_le_bytes());
            }
        }

        for (off, target_block, kind, opcode) in patches {
            let target_off = mf
                .blocks
                .iter()
                .take(target_block)
                .map(|b| b.instrs.len() * 4)
                .sum::<usize>();
            let rel = (target_off as i64) - (off as i64);
            let old = u32::from_le_bytes([data[off], data[off + 1], data[off + 2], data[off + 3]]);
            let new = match kind {
                PatchKind::Branch13 => {
                    let rs2 = ((old >> 20) & 0x1F) as u8;
                    let rs1 = ((old >> 15) & 0x1F) as u8;
                    let f3 = (old >> 12) & 0x7;
                    enc_b(rel as i32, rs2, rs1, f3, 0x63)
                }
                PatchKind::Jal21 => {
                    let rd = ((old >> 7) & 0x1F) as u8;
                    enc_j(rel as i32, rd, 0x6F)
                }
            };
            data[off..off + 4].copy_from_slice(&new.to_le_bytes());
            let _ = opcode;
        }

        Section {
            name: ".text".into(),
            data,
            relocs: Vec::new(),
            debug_rows,
        }
    }

    fn object_format(&self) -> ObjectFormat {
        self.format
    }

    fn elf_machine(&self) -> u16 {
        243 // EM_RISCV
    }
}

#[derive(Clone, Copy)]
enum PatchKind {
    Branch13,
    Jal21,
}

fn patch_from_instr(instr: &MInstr) -> Option<(usize, PatchKind)> {
    let op = instr.opcode;
    if matches!(op, BEQ | BNE | BLT | BGE | BLTU | BGEU) {
        if let Some(MOperand::Block(b)) = instr.operands.get(2) {
            return Some((*b, PatchKind::Branch13));
        }
    }
    if op == JAL {
        if let Some(MOperand::Block(b)) = instr.operands.first() {
            return Some((*b, PatchKind::Jal21));
        }
    }
    None
}

fn reg_of_dst(v: VReg) -> u8 {
    (v.0 & 0x1F) as u8
}

fn reg_of_op(op: &MOperand) -> Option<u8> {
    match op {
        MOperand::PReg(PReg(r)) => Some(*r & 0x1F),
        MOperand::VReg(VReg(v)) => Some((v & 0x1F) as u8),
        _ => None,
    }
}

/// Extract the 5-bit hardware FP register number from dst VReg (post-regalloc).
/// After regalloc, FP VRegs have PReg(32..63) baked into the low 8 bits of the VReg.
fn fp_reg_of_dst(v: VReg) -> u8 {
    // Post-allocation: v.0 = PReg.0 (8-bit physical reg number stored in u32).
    // FP regs: PReg(32..63) → fp_enc = PReg.0 - 32.
    let raw = (v.0 & 0xFF) as u8;
    if raw >= 32 {
        raw - 32
    } else {
        raw
    }
}

/// Extract the 5-bit hardware FP register number from a PReg operand.
fn fp_reg_of_op(op: &MOperand) -> Option<u8> {
    match op {
        MOperand::PReg(r) if is_fp_reg(*r) => Some(fp_enc(*r)),
        MOperand::PReg(PReg(r)) => Some((*r) & 0x1F),
        MOperand::VReg(VReg(v)) => {
            // Post-regalloc: VReg carries the physical register number.
            let raw = (*v & 0xFF) as u8;
            // If it was an FP VReg, the bit pattern in the class-bit was stripped
            // by apply_allocation, so use raw directly.
            if raw >= 32 {
                Some(raw - 32)
            } else {
                Some(raw)
            }
        }
        _ => None,
    }
}

fn imm_of_op(op: Option<&MOperand>) -> i32 {
    if let Some(MOperand::Imm(v)) = op {
        *v as i32
    } else {
        0
    }
}

fn enc_r(f7: u32, rs2: u8, rs1: u8, f3: u32, rd: u8, opc: u32) -> u32 {
    (f7 << 25) | ((rs2 as u32) << 20) | ((rs1 as u32) << 15) | (f3 << 12) | ((rd as u32) << 7) | opc
}

fn enc_i(imm: i32, rs1: u8, f3: u32, rd: u8, opc: u32) -> u32 {
    let i = (imm as u32) & 0xFFF;
    (i << 20) | ((rs1 as u32) << 15) | (f3 << 12) | ((rd as u32) << 7) | opc
}

fn enc_s(imm: i32, rs2: u8, rs1: u8, f3: u32, opc: u32) -> u32 {
    let i = (imm as u32) & 0xFFF;
    let hi = (i >> 5) & 0x7F;
    let lo = i & 0x1F;
    (hi << 25) | ((rs2 as u32) << 20) | ((rs1 as u32) << 15) | (f3 << 12) | (lo << 7) | opc
}

fn enc_b(imm: i32, rs2: u8, rs1: u8, f3: u32, opc: u32) -> u32 {
    let i = (imm as u32) & 0x1FFF;
    let b12 = (i >> 12) & 0x1;
    let b10_5 = (i >> 5) & 0x3F;
    let b4_1 = (i >> 1) & 0xF;
    let b11 = (i >> 11) & 0x1;
    (b12 << 31)
        | (b10_5 << 25)
        | ((rs2 as u32) << 20)
        | ((rs1 as u32) << 15)
        | (f3 << 12)
        | (b4_1 << 8)
        | (b11 << 7)
        | opc
}

fn enc_u(imm: i32, rd: u8, opc: u32) -> u32 {
    ((imm as u32) & 0xFFFFF000) | ((rd as u32) << 7) | opc
}

fn enc_j(imm: i32, rd: u8, opc: u32) -> u32 {
    let i = (imm as u32) & 0x1FFFFF;
    let b20 = (i >> 20) & 0x1;
    let b10_1 = (i >> 1) & 0x3FF;
    let b11 = (i >> 11) & 0x1;
    let b19_12 = (i >> 12) & 0xFF;
    (b20 << 31) | (b10_1 << 21) | (b11 << 20) | (b19_12 << 12) | ((rd as u32) << 7) | opc
}

/// Encode an A-extension AMO instruction (R4-type: funct5|aq|rl|rs2|rs1|funct3|rd|opcode=0x2F).
fn enc_amo(funct5: u32, aq: bool, rl: bool, rs2: u32, rs1: u32, funct3: u32, rd: u32) -> u32 {
    ((funct5 & 0x1F) << 27)
        | ((aq as u32) << 26)
        | ((rl as u32) << 25)
        | ((rs2 & 0x1F) << 20)
        | ((rs1 & 0x1F) << 15)
        | ((funct3 & 0x7) << 12)
        | ((rd & 0x1F) << 7)
        | 0x2F
}

fn encode_instr(instr: &MInstr) -> u32 {
    let rd = instr.dst.map(reg_of_dst).unwrap_or(0);
    let rs1 = instr.operands.first().and_then(reg_of_op).unwrap_or(0);
    let rs2 = instr.operands.get(1).and_then(reg_of_op).unwrap_or(0);

    match instr.opcode {
        NOP => 0x00000013,
        MOV_RR => enc_i(0, rs1, 0x0, rd, 0x13),
        MOV_IMM => enc_i(imm_of_op(instr.operands.first()), 0, 0x0, rd, 0x13),
        MOV_PR => {
            // operands[0]=PReg(dst), operands[1]=src
            let d = instr.operands.first().and_then(reg_of_op).unwrap_or(0);
            let s = instr.operands.get(1).and_then(reg_of_op).unwrap_or(0);
            enc_i(0, s, 0x0, d, 0x13)
        }

        ADD_RR => enc_r(0x00, rs2, rs1, 0x0, rd, 0x33),
        SUB_RR => enc_r(0x20, rs2, rs1, 0x0, rd, 0x33),
        MUL_RR => enc_r(0x01, rs2, rs1, 0x0, rd, 0x33),
        DIV_RR => enc_r(0x01, rs2, rs1, 0x4, rd, 0x33),
        UDIV_RR => enc_r(0x01, rs2, rs1, 0x5, rd, 0x33),
        REM_RR => enc_r(0x01, rs2, rs1, 0x6, rd, 0x33),
        UREM_RR => enc_r(0x01, rs2, rs1, 0x7, rd, 0x33),
        AND_RR => enc_r(0x00, rs2, rs1, 0x7, rd, 0x33),
        OR_RR => enc_r(0x00, rs2, rs1, 0x6, rd, 0x33),
        XOR_RR => enc_r(0x00, rs2, rs1, 0x4, rd, 0x33),
        SLL_RR => enc_r(0x00, rs2, rs1, 0x1, rd, 0x33),
        SRL_RR => enc_r(0x00, rs2, rs1, 0x5, rd, 0x33),
        SRA_RR => enc_r(0x20, rs2, rs1, 0x5, rd, 0x33),
        SLT_RR => enc_r(0x00, rs2, rs1, 0x2, rd, 0x33),
        SLTU_RR => enc_r(0x00, rs2, rs1, 0x3, rd, 0x33),

        ADDI => enc_i(imm_of_op(instr.operands.get(1)), rs1, 0x0, rd, 0x13),
        XORI => enc_i(imm_of_op(instr.operands.get(1)), rs1, 0x4, rd, 0x13),
        SLTIU => enc_i(imm_of_op(instr.operands.get(1)), rs1, 0x3, rd, 0x13),

        LW => enc_i(imm_of_op(instr.operands.get(2)), rs1, 0x2, rd, 0x03),
        LD => enc_i(imm_of_op(instr.operands.get(2)), rs1, 0x3, rd, 0x03),
        SW => enc_s(imm_of_op(instr.operands.get(2)), rs2, rs1, 0x2, 0x23),
        SD => enc_s(imm_of_op(instr.operands.get(2)), rs2, rs1, 0x3, 0x23),

        BEQ => enc_b(imm_of_op(instr.operands.get(2)), rs2, rs1, 0x0, 0x63),
        BNE => enc_b(imm_of_op(instr.operands.get(2)), rs2, rs1, 0x1, 0x63),
        BLT => enc_b(imm_of_op(instr.operands.get(2)), rs2, rs1, 0x4, 0x63),
        BGE => enc_b(imm_of_op(instr.operands.get(2)), rs2, rs1, 0x5, 0x63),
        BLTU => enc_b(imm_of_op(instr.operands.get(2)), rs2, rs1, 0x6, 0x63),
        BGEU => enc_b(imm_of_op(instr.operands.get(2)), rs2, rs1, 0x7, 0x63),

        JAL => enc_j(imm_of_op(instr.operands.first()), rd, 0x6F),
        JALR => {
            let d = instr
                .dst
                .map(reg_of_dst)
                .unwrap_or_else(|| instr.operands.first().and_then(reg_of_op).unwrap_or(0));
            let base_idx = if instr.dst.is_some() { 0 } else { 1 };
            let base = instr
                .operands
                .get(base_idx)
                .and_then(reg_of_op)
                .unwrap_or(0);
            let imm = imm_of_op(instr.operands.get(base_idx + 1));
            enc_i(imm, base, 0x0, d, 0x67)
        }
        RET => enc_i(0, 1, 0x0, 0, 0x67),

        LUI => enc_u(imm_of_op(instr.operands.first()), rd, 0x37),
        AUIPC => enc_u(imm_of_op(instr.operands.first()), rd, 0x17),

        // ── A-extension atomics ────────────────────────────────────────────
        FENCE => 0x0FF0_000F, // fence iorw,iorw (seq_cst)

        // LR.W / LR.D: rs2 = 0 (no second register operand)
        LR_W => enc_amo(0b00010, true, true, 0, rs1 as u32, 0b010, rd as u32),
        LR_D => enc_amo(0b00010, true, true, 0, rs1 as u32, 0b011, rd as u32),

        // SC.W / SC.D: operands[0]=rs1(ptr), operands[1]=rs2(new value)
        SC_W => enc_amo(
            0b00011, true, true, rs2 as u32, rs1 as u32, 0b010, rd as u32,
        ),
        SC_D => enc_amo(
            0b00011, true, true, rs2 as u32, rs1 as u32, 0b011, rd as u32,
        ),

        // AMO word variants
        AMOADD_W => enc_amo(
            0b00000, true, true, rs2 as u32, rs1 as u32, 0b010, rd as u32,
        ),
        AMOSWAP_W => enc_amo(
            0b00001, true, true, rs2 as u32, rs1 as u32, 0b010, rd as u32,
        ),
        AMOXOR_W => enc_amo(
            0b00100, true, true, rs2 as u32, rs1 as u32, 0b010, rd as u32,
        ),
        AMOAND_W => enc_amo(
            0b01100, true, true, rs2 as u32, rs1 as u32, 0b010, rd as u32,
        ),
        AMOOR_W => enc_amo(
            0b01000, true, true, rs2 as u32, rs1 as u32, 0b010, rd as u32,
        ),
        AMOMIN_W => enc_amo(
            0b10000, true, true, rs2 as u32, rs1 as u32, 0b010, rd as u32,
        ),
        AMOMAX_W => enc_amo(
            0b10100, true, true, rs2 as u32, rs1 as u32, 0b010, rd as u32,
        ),
        AMOMINU_W => enc_amo(
            0b11000, true, true, rs2 as u32, rs1 as u32, 0b010, rd as u32,
        ),
        AMOMAXU_W => enc_amo(
            0b11100, true, true, rs2 as u32, rs1 as u32, 0b010, rd as u32,
        ),

        // AMO doubleword variants
        AMOADD_D => enc_amo(
            0b00000, true, true, rs2 as u32, rs1 as u32, 0b011, rd as u32,
        ),
        AMOSWAP_D => enc_amo(
            0b00001, true, true, rs2 as u32, rs1 as u32, 0b011, rd as u32,
        ),
        AMOXOR_D => enc_amo(
            0b00100, true, true, rs2 as u32, rs1 as u32, 0b011, rd as u32,
        ),
        AMOAND_D => enc_amo(
            0b01100, true, true, rs2 as u32, rs1 as u32, 0b011, rd as u32,
        ),
        AMOOR_D => enc_amo(
            0b01000, true, true, rs2 as u32, rs1 as u32, 0b011, rd as u32,
        ),

        // ── non-promotable alloca frame-slot access ────────────────────────
        // ADDI_FP_SLOT: addi rd, s0(x8), -(slot_idx+1)*8
        // s0 is the RISC-V frame pointer (saved s0/ra in prologue).
        ADDI_FP_SLOT => {
            let slot_idx = imm_of_op(instr.operands.first());
            let imm = -(slot_idx + 1) * 8;
            enc_i(imm, 8 /* s0/fp */, 0x0, rd, 0x13)
        }
        // LD_REG: ld rd, 0(rs1)  — load 64-bit word from pointer reg with offset 0
        LD_REG => enc_i(0, rs1, 0x3, rd, 0x03),
        // SD_REG: sd rs2, 0(rs1)  — store 64-bit word through pointer reg
        SD_REG => enc_s(0, rs2, rs1, 0x3, 0x23),

        // ── F+D extension (scalar FP) ──────────────────────────────────────
        //
        // FP R-type format: funct7(7) | rs2(5) | rs1(5) | funct3(3) | rd(5) | opcode(7)
        // opcode = 0x53 for all FP arithmetic.  funct3 = 0b111 (RNE rounding mode).
        // Register fields use the 5-bit FP register number (0-31).
        //
        // After register allocation, `instr.dst` holds VReg(PReg.0 as u32) where
        // PReg.0 is in 32-63 for FP registers.  `fp_reg_of_dst` converts to 0-31.
        // Similarly, operands are PReg(32..63) for FP registers.

        // fadd.d rd, rs1, rs2 — funct7=0x01, funct3=RNE=0x7
        FADD_D => {
            let rd = fp_reg_of_dst(instr.dst.unwrap_or(VReg(0)));
            let rs1 = instr.operands.first().and_then(fp_reg_of_op).unwrap_or(0);
            let rs2 = instr.operands.get(1).and_then(fp_reg_of_op).unwrap_or(0);
            enc_r(0x01, rs2, rs1, 0x7, rd, 0x53)
        }
        // fsub.d — funct7=0x05
        FSUB_D => {
            let rd = fp_reg_of_dst(instr.dst.unwrap_or(VReg(0)));
            let rs1 = instr.operands.first().and_then(fp_reg_of_op).unwrap_or(0);
            let rs2 = instr.operands.get(1).and_then(fp_reg_of_op).unwrap_or(0);
            enc_r(0x05, rs2, rs1, 0x7, rd, 0x53)
        }
        // fmul.d — funct7=0x09
        FMUL_D => {
            let rd = fp_reg_of_dst(instr.dst.unwrap_or(VReg(0)));
            let rs1 = instr.operands.first().and_then(fp_reg_of_op).unwrap_or(0);
            let rs2 = instr.operands.get(1).and_then(fp_reg_of_op).unwrap_or(0);
            enc_r(0x09, rs2, rs1, 0x7, rd, 0x53)
        }
        // fdiv.d — funct7=0x0D
        FDIV_D => {
            let rd = fp_reg_of_dst(instr.dst.unwrap_or(VReg(0)));
            let rs1 = instr.operands.first().and_then(fp_reg_of_op).unwrap_or(0);
            let rs2 = instr.operands.get(1).and_then(fp_reg_of_op).unwrap_or(0);
            enc_r(0x0D, rs2, rs1, 0x7, rd, 0x53)
        }
        // fsqrt.d — funct7=0x2D, rs2=0
        FSQRT_D => {
            let rd = fp_reg_of_dst(instr.dst.unwrap_or(VReg(0)));
            let rs1 = instr.operands.first().and_then(fp_reg_of_op).unwrap_or(0);
            enc_r(0x2D, 0, rs1, 0x7, rd, 0x53)
        }
        // fsgnjn.d rd, rs1, rs1 — fneg: funct7=0x21, funct3=0x1, rs2=rs1
        FNEG_D => {
            let rd = fp_reg_of_dst(instr.dst.unwrap_or(VReg(0)));
            let rs1 = instr.operands.first().and_then(fp_reg_of_op).unwrap_or(0);
            // rs2 = rs1 (FSGNJN rd, rs, rs)
            enc_r(0x21, rs1, rs1, 0x1, rd, 0x53)
        }
        // feq.d rd, rs1, rs2 — funct7=0x51, funct3=0x2; result is integer
        FCMP_EQ_D => {
            let rd = reg_of_dst(instr.dst.unwrap_or(VReg(0)));
            let rs1 = instr.operands.first().and_then(fp_reg_of_op).unwrap_or(0);
            let rs2 = instr.operands.get(1).and_then(fp_reg_of_op).unwrap_or(0);
            enc_r(0x51, rs2, rs1, 0x2, rd, 0x53)
        }
        // flt.d rd, rs1, rs2 — funct7=0x51, funct3=0x1
        FCMP_LT_D => {
            let rd = reg_of_dst(instr.dst.unwrap_or(VReg(0)));
            let rs1 = instr.operands.first().and_then(fp_reg_of_op).unwrap_or(0);
            let rs2 = instr.operands.get(1).and_then(fp_reg_of_op).unwrap_or(0);
            enc_r(0x51, rs2, rs1, 0x1, rd, 0x53)
        }
        // fle.d rd, rs1, rs2 — funct7=0x51, funct3=0x0
        FCMP_LE_D => {
            let rd = reg_of_dst(instr.dst.unwrap_or(VReg(0)));
            let rs1 = instr.operands.first().and_then(fp_reg_of_op).unwrap_or(0);
            let rs2 = instr.operands.get(1).and_then(fp_reg_of_op).unwrap_or(0);
            enc_r(0x51, rs2, rs1, 0x0, rd, 0x53)
        }
        // fcvt.w.d rd, rs1 — funct7=0x61, rs2=0, rounding=RTZ=0x1
        FCVT_W_D => {
            let rd = reg_of_dst(instr.dst.unwrap_or(VReg(0)));
            let rs1 = instr.operands.first().and_then(fp_reg_of_op).unwrap_or(0);
            enc_r(0x61, 0, rs1, 0x1, rd, 0x53)
        }
        // fcvt.l.d rd, rs1 — funct7=0x61, rs2=2, rounding=RTZ=0x1
        FCVT_L_D => {
            let rd = reg_of_dst(instr.dst.unwrap_or(VReg(0)));
            let rs1 = instr.operands.first().and_then(fp_reg_of_op).unwrap_or(0);
            enc_r(0x61, 2, rs1, 0x1, rd, 0x53)
        }
        // fcvt.d.w rd, rs1 — funct7=0x69, rs2=0
        FCVT_D_W => {
            let rd = fp_reg_of_dst(instr.dst.unwrap_or(VReg(0)));
            let rs1 = instr.operands.first().and_then(reg_of_op).unwrap_or(0);
            enc_r(0x69, 0, rs1, 0x0, rd, 0x53)
        }
        // fcvt.d.l rd, rs1 — funct7=0x69, rs2=2
        FCVT_D_L => {
            let rd = fp_reg_of_dst(instr.dst.unwrap_or(VReg(0)));
            let rs1 = instr.operands.first().and_then(reg_of_op).unwrap_or(0);
            enc_r(0x69, 2, rs1, 0x0, rd, 0x53)
        }
        // fsgnj.d rd, rs1, rs1 — FP register copy; funct7=0x11, funct3=0x0, rs2=rs1
        FMV_D_D => {
            let rd = fp_reg_of_dst(instr.dst.unwrap_or(VReg(0)));
            let rs1 = instr.operands.first().and_then(fp_reg_of_op).unwrap_or(0);
            enc_r(0x11, rs1, rs1, 0x0, rd, 0x53)
        }
        // fld rd, imm(rs1) — I-type: opcode=0x07, funct3=0x3
        FLD => {
            let rd = fp_reg_of_dst(instr.dst.unwrap_or(VReg(0)));
            let rs1 = instr.operands.first().and_then(reg_of_op).unwrap_or(0);
            let imm = imm_of_op(instr.operands.get(1));
            enc_i(imm, rs1, 0x3, rd, 0x07)
        }
        // fsd rs2, imm(rs1) — S-type: opcode=0x27, funct3=0x3
        FSD => {
            let rs1 = instr.operands.first().and_then(reg_of_op).unwrap_or(0);
            let rs2 = instr.operands.get(1).and_then(fp_reg_of_op).unwrap_or(0);
            let imm = imm_of_op(instr.operands.get(2));
            enc_s(imm, rs2, rs1, 0x3, 0x27)
        }
        // FP_LOAD_MR: FP spill reload — FLD Fd, slot*8(sp)  (same encoding as FLD)
        // Layout: dst=VReg(FPReg), operands=[Imm(slot)]
        // The frame pointer (x8/s0) is used for frame-relative addressing.
        FP_LOAD_MR => {
            let rd = fp_reg_of_dst(instr.dst.unwrap_or(VReg(0)));
            let slot = imm_of_op(instr.operands.first());
            let byte_off = slot * 8; // slot index → byte offset
                                     // Use x8 (s0/fp) as frame pointer, same as integer spill reloads.
            enc_i(byte_off, 8, 0x3, rd, 0x07)
        }
        // FP_STORE_RM: FP spill save — FSD Fs, slot*8(sp)
        // Layout: dst=None, operands=[Imm(slot), VReg(src)]
        FP_STORE_RM => {
            let slot = imm_of_op(instr.operands.first());
            let byte_off = slot * 8;
            let rs2 = instr.operands.get(1).and_then(fp_reg_of_op).unwrap_or(0);
            enc_s(byte_off, rs2, 8, 0x3, 0x27)
        }

        // ── Single-precision (F extension) ────────────────────────────────
        // fadd.s — funct7=0x00
        FADD_S => {
            let rd = fp_reg_of_dst(instr.dst.unwrap_or(VReg(0)));
            let rs1 = instr.operands.first().and_then(fp_reg_of_op).unwrap_or(0);
            let rs2 = instr.operands.get(1).and_then(fp_reg_of_op).unwrap_or(0);
            enc_r(0x00, rs2, rs1, 0x7, rd, 0x53)
        }
        // fsub.s — funct7=0x04
        FSUB_S => {
            let rd = fp_reg_of_dst(instr.dst.unwrap_or(VReg(0)));
            let rs1 = instr.operands.first().and_then(fp_reg_of_op).unwrap_or(0);
            let rs2 = instr.operands.get(1).and_then(fp_reg_of_op).unwrap_or(0);
            enc_r(0x04, rs2, rs1, 0x7, rd, 0x53)
        }
        // fmul.s — funct7=0x08
        FMUL_S => {
            let rd = fp_reg_of_dst(instr.dst.unwrap_or(VReg(0)));
            let rs1 = instr.operands.first().and_then(fp_reg_of_op).unwrap_or(0);
            let rs2 = instr.operands.get(1).and_then(fp_reg_of_op).unwrap_or(0);
            enc_r(0x08, rs2, rs1, 0x7, rd, 0x53)
        }
        // fdiv.s — funct7=0x0C
        FDIV_S => {
            let rd = fp_reg_of_dst(instr.dst.unwrap_or(VReg(0)));
            let rs1 = instr.operands.first().and_then(fp_reg_of_op).unwrap_or(0);
            let rs2 = instr.operands.get(1).and_then(fp_reg_of_op).unwrap_or(0);
            enc_r(0x0C, rs2, rs1, 0x7, rd, 0x53)
        }
        // fsgnjn.s rd, rs, rs (fneg single) — funct7=0x20, funct3=0x1
        FNEG_S => {
            let rd = fp_reg_of_dst(instr.dst.unwrap_or(VReg(0)));
            let rs1 = instr.operands.first().and_then(fp_reg_of_op).unwrap_or(0);
            enc_r(0x20, rs1, rs1, 0x1, rd, 0x53)
        }
        // feq.s — funct7=0x50, funct3=0x2
        FCMP_EQ_S => {
            let rd = reg_of_dst(instr.dst.unwrap_or(VReg(0)));
            let rs1 = instr.operands.first().and_then(fp_reg_of_op).unwrap_or(0);
            let rs2 = instr.operands.get(1).and_then(fp_reg_of_op).unwrap_or(0);
            enc_r(0x50, rs2, rs1, 0x2, rd, 0x53)
        }
        // flt.s — funct7=0x50, funct3=0x1
        FCMP_LT_S => {
            let rd = reg_of_dst(instr.dst.unwrap_or(VReg(0)));
            let rs1 = instr.operands.first().and_then(fp_reg_of_op).unwrap_or(0);
            let rs2 = instr.operands.get(1).and_then(fp_reg_of_op).unwrap_or(0);
            enc_r(0x50, rs2, rs1, 0x1, rd, 0x53)
        }
        // fle.s — funct7=0x50, funct3=0x0
        FCMP_LE_S => {
            let rd = reg_of_dst(instr.dst.unwrap_or(VReg(0)));
            let rs1 = instr.operands.first().and_then(fp_reg_of_op).unwrap_or(0);
            let rs2 = instr.operands.get(1).and_then(fp_reg_of_op).unwrap_or(0);
            enc_r(0x50, rs2, rs1, 0x0, rd, 0x53)
        }
        // fcvt.w.s — funct7=0x60, rs2=0, rounding=RTZ
        FCVT_W_S => {
            let rd = reg_of_dst(instr.dst.unwrap_or(VReg(0)));
            let rs1 = instr.operands.first().and_then(fp_reg_of_op).unwrap_or(0);
            enc_r(0x60, 0, rs1, 0x1, rd, 0x53)
        }
        // fcvt.l.s — funct7=0x60, rs2=2, rounding=RTZ
        FCVT_L_S => {
            let rd = reg_of_dst(instr.dst.unwrap_or(VReg(0)));
            let rs1 = instr.operands.first().and_then(fp_reg_of_op).unwrap_or(0);
            enc_r(0x60, 2, rs1, 0x1, rd, 0x53)
        }
        // fcvt.s.w — funct7=0x68, rs2=0
        FCVT_S_W => {
            let rd = fp_reg_of_dst(instr.dst.unwrap_or(VReg(0)));
            let rs1 = instr.operands.first().and_then(reg_of_op).unwrap_or(0);
            enc_r(0x68, 0, rs1, 0x0, rd, 0x53)
        }
        // fcvt.s.l — funct7=0x68, rs2=2
        FCVT_S_L => {
            let rd = fp_reg_of_dst(instr.dst.unwrap_or(VReg(0)));
            let rs1 = instr.operands.first().and_then(reg_of_op).unwrap_or(0);
            enc_r(0x68, 2, rs1, 0x0, rd, 0x53)
        }
        // flw rd, imm(rs1) — I-type: opcode=0x07, funct3=0x2
        FLW => {
            let rd = fp_reg_of_dst(instr.dst.unwrap_or(VReg(0)));
            let rs1 = instr.operands.first().and_then(reg_of_op).unwrap_or(0);
            let imm = imm_of_op(instr.operands.get(1));
            enc_i(imm, rs1, 0x2, rd, 0x07)
        }
        // fsw rs2, imm(rs1) — S-type: opcode=0x27, funct3=0x2
        FSW => {
            let rs1 = instr.operands.first().and_then(reg_of_op).unwrap_or(0);
            let rs2 = instr.operands.get(1).and_then(fp_reg_of_op).unwrap_or(0);
            let imm = imm_of_op(instr.operands.get(2));
            enc_s(imm, rs2, rs1, 0x2, 0x27)
        }

        _ => panic!("unsupported RISC-V opcode {:?}", instr.opcode),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use llvm_codegen::emit::{emit_object, ObjectFormat};
    use llvm_codegen::isel::{MInstr, MachineFunction};

    fn rr(op: llvm_codegen::isel::MOpcode) -> MInstr {
        MInstr::new(op)
            .with_dst(VReg(3))
            .with_preg(PReg(1))
            .with_preg(PReg(2))
    }

    #[test]
    fn enc_r_add() {
        assert_eq!(encode_instr(&rr(ADD_RR)), enc_r(0x00, 2, 1, 0, 3, 0x33));
    }
    #[test]
    fn enc_r_sub() {
        assert_eq!(encode_instr(&rr(SUB_RR)), enc_r(0x20, 2, 1, 0, 3, 0x33));
    }
    #[test]
    fn enc_r_mul() {
        assert_eq!(encode_instr(&rr(MUL_RR)), enc_r(0x01, 2, 1, 0, 3, 0x33));
    }
    #[test]
    fn enc_r_div() {
        assert_eq!(encode_instr(&rr(DIV_RR)), enc_r(0x01, 2, 1, 4, 3, 0x33));
    }
    #[test]
    fn enc_r_udiv() {
        assert_eq!(encode_instr(&rr(UDIV_RR)), enc_r(0x01, 2, 1, 5, 3, 0x33));
    }
    #[test]
    fn enc_r_rem() {
        assert_eq!(encode_instr(&rr(REM_RR)), enc_r(0x01, 2, 1, 6, 3, 0x33));
    }
    #[test]
    fn enc_r_urem() {
        assert_eq!(encode_instr(&rr(UREM_RR)), enc_r(0x01, 2, 1, 7, 3, 0x33));
    }
    #[test]
    fn enc_r_and() {
        assert_eq!(encode_instr(&rr(AND_RR)), enc_r(0x00, 2, 1, 7, 3, 0x33));
    }
    #[test]
    fn enc_r_or() {
        assert_eq!(encode_instr(&rr(OR_RR)), enc_r(0x00, 2, 1, 6, 3, 0x33));
    }
    #[test]
    fn enc_r_xor() {
        assert_eq!(encode_instr(&rr(XOR_RR)), enc_r(0x00, 2, 1, 4, 3, 0x33));
    }
    #[test]
    fn enc_r_sll() {
        assert_eq!(encode_instr(&rr(SLL_RR)), enc_r(0x00, 2, 1, 1, 3, 0x33));
    }
    #[test]
    fn enc_r_srl() {
        assert_eq!(encode_instr(&rr(SRL_RR)), enc_r(0x00, 2, 1, 5, 3, 0x33));
    }
    #[test]
    fn enc_r_sra() {
        assert_eq!(encode_instr(&rr(SRA_RR)), enc_r(0x20, 2, 1, 5, 3, 0x33));
    }
    #[test]
    fn enc_r_slt() {
        assert_eq!(encode_instr(&rr(SLT_RR)), enc_r(0x00, 2, 1, 2, 3, 0x33));
    }
    #[test]
    fn enc_r_sltu() {
        assert_eq!(encode_instr(&rr(SLTU_RR)), enc_r(0x00, 2, 1, 3, 3, 0x33));
    }

    #[test]
    fn enc_mov_rr() {
        let mi = MInstr::new(MOV_RR).with_dst(VReg(3)).with_preg(PReg(1));
        assert_eq!(encode_instr(&mi), enc_i(0, 1, 0, 3, 0x13));
    }
    #[test]
    fn enc_mov_imm() {
        let mi = MInstr::new(MOV_IMM).with_dst(VReg(3)).with_imm(9);
        assert_eq!(encode_instr(&mi), enc_i(9, 0, 0, 3, 0x13));
    }
    #[test]
    fn enc_mov_pr() {
        let mi = MInstr::new(MOV_PR).with_preg(PReg(10)).with_preg(PReg(5));
        assert_eq!(encode_instr(&mi), enc_i(0, 5, 0, 10, 0x13));
    }

    #[test]
    fn enc_i_addi() {
        let mi = MInstr::new(ADDI)
            .with_dst(VReg(1))
            .with_preg(PReg(2))
            .with_imm(7);
        assert_eq!(encode_instr(&mi), enc_i(7, 2, 0, 1, 0x13));
    }
    #[test]
    fn enc_i_xori() {
        let mi = MInstr::new(XORI)
            .with_dst(VReg(1))
            .with_preg(PReg(2))
            .with_imm(1);
        assert_eq!(encode_instr(&mi), enc_i(1, 2, 4, 1, 0x13));
    }
    #[test]
    fn enc_i_sltiu() {
        let mi = MInstr::new(SLTIU)
            .with_dst(VReg(1))
            .with_preg(PReg(2))
            .with_imm(1);
        assert_eq!(encode_instr(&mi), enc_i(1, 2, 3, 1, 0x13));
    }

    #[test]
    fn enc_i_lw() {
        let mi = MInstr::new(LW)
            .with_dst(VReg(3))
            .with_preg(PReg(1))
            .with_preg(PReg(0))
            .with_imm(16);
        assert_eq!(encode_instr(&mi), enc_i(16, 1, 2, 3, 0x03));
    }
    #[test]
    fn enc_i_ld() {
        let mi = MInstr::new(LD)
            .with_dst(VReg(3))
            .with_preg(PReg(1))
            .with_preg(PReg(0))
            .with_imm(24);
        assert_eq!(encode_instr(&mi), enc_i(24, 1, 3, 3, 0x03));
    }
    #[test]
    fn enc_s_sw() {
        let mi = MInstr::new(SW)
            .with_preg(PReg(1))
            .with_preg(PReg(2))
            .with_imm(20);
        assert_eq!(encode_instr(&mi), enc_s(20, 2, 1, 2, 0x23));
    }
    #[test]
    fn enc_s_sd() {
        let mi = MInstr::new(SD)
            .with_preg(PReg(1))
            .with_preg(PReg(2))
            .with_imm(28);
        assert_eq!(encode_instr(&mi), enc_s(28, 2, 1, 3, 0x23));
    }

    #[test]
    fn enc_b_beq() {
        let mi = MInstr::new(BEQ)
            .with_preg(PReg(1))
            .with_preg(PReg(2))
            .with_imm(16);
        assert_eq!(encode_instr(&mi), enc_b(16, 2, 1, 0, 0x63));
    }
    #[test]
    fn enc_b_bne() {
        let mi = MInstr::new(BNE)
            .with_preg(PReg(1))
            .with_preg(PReg(2))
            .with_imm(16);
        assert_eq!(encode_instr(&mi), enc_b(16, 2, 1, 1, 0x63));
    }
    #[test]
    fn enc_b_blt() {
        let mi = MInstr::new(BLT)
            .with_preg(PReg(1))
            .with_preg(PReg(2))
            .with_imm(16);
        assert_eq!(encode_instr(&mi), enc_b(16, 2, 1, 4, 0x63));
    }
    #[test]
    fn enc_b_bge() {
        let mi = MInstr::new(BGE)
            .with_preg(PReg(1))
            .with_preg(PReg(2))
            .with_imm(16);
        assert_eq!(encode_instr(&mi), enc_b(16, 2, 1, 5, 0x63));
    }
    #[test]
    fn enc_b_bltu() {
        let mi = MInstr::new(BLTU)
            .with_preg(PReg(1))
            .with_preg(PReg(2))
            .with_imm(16);
        assert_eq!(encode_instr(&mi), enc_b(16, 2, 1, 6, 0x63));
    }
    #[test]
    fn enc_b_bgeu() {
        let mi = MInstr::new(BGEU)
            .with_preg(PReg(1))
            .with_preg(PReg(2))
            .with_imm(16);
        assert_eq!(encode_instr(&mi), enc_b(16, 2, 1, 7, 0x63));
    }

    #[test]
    fn enc_j_jal() {
        let mi = MInstr::new(JAL).with_dst(VReg(1)).with_imm(32);
        assert_eq!(encode_instr(&mi), enc_j(32, 1, 0x6F));
    }
    #[test]
    fn enc_i_jalr() {
        let mi = MInstr::new(JALR)
            .with_dst(VReg(1))
            .with_preg(PReg(2))
            .with_imm(12);
        assert_eq!(encode_instr(&mi), enc_i(12, 2, 0, 1, 0x67));
    }
    #[test]
    fn enc_ret() {
        assert_eq!(encode_instr(&MInstr::new(RET)), enc_i(0, 1, 0, 0, 0x67));
    }

    #[test]
    fn enc_u_lui() {
        let mi = MInstr::new(LUI).with_dst(VReg(1)).with_imm(0x12345000);
        assert_eq!(encode_instr(&mi), enc_u(0x12345000, 1, 0x37));
    }
    #[test]
    fn enc_u_auipc() {
        let mi = MInstr::new(AUIPC).with_dst(VReg(1)).with_imm(0x1000);
        assert_eq!(encode_instr(&mi), enc_u(0x1000, 1, 0x17));
    }

    #[test]
    fn helper_i_sign_wrap() {
        assert_eq!(enc_i(-1, 1, 0, 2, 0x13) >> 20, 0xFFF);
    }
    #[test]
    fn helper_s_splits_imm() {
        assert_eq!((enc_s(0x7F, 2, 1, 0, 0x23) >> 7) & 0x1F, 0x1F);
    }
    #[test]
    fn helper_b_encodes_bit11() {
        assert_eq!((enc_b(0x800, 2, 1, 0, 0x63) >> 7) & 1, 1);
    }
    #[test]
    fn helper_j_encodes_bit20() {
        assert_eq!((enc_j(0x100000, 1, 0x6F) >> 31) & 1, 1);
    }
    #[test]
    fn helper_u_keeps_upper_20() {
        assert_eq!(
            enc_u(0xABCD1000u32 as i32, 1, 0x37) & 0xFFFFF000,
            0xABCD1000
        );
    }

    #[test]
    fn emitter_outputs_words() {
        let mut mf = MachineFunction::new("f".into());
        let b = mf.add_block("entry");
        mf.push(b, rr(ADD_RR));
        mf.push(b, MInstr::new(RET));
        let mut e = RiscVEmitter::new(ObjectFormat::Elf);
        let sec = e.emit_function(&mf);
        assert_eq!(sec.data.len(), 8);
        assert!(sec.relocs.is_empty());
    }

    #[test]
    fn emitter_branch_patch() {
        let mut mf = MachineFunction::new("f".into());
        let b0 = mf.add_block("b0");
        let b1 = mf.add_block("b1");
        mf.push(b0, MInstr::new(JAL).with_block(b1));
        mf.push(b1, MInstr::new(RET));
        let mut e = RiscVEmitter::new(ObjectFormat::Elf);
        let sec = e.emit_function(&mf);
        let w0 = u32::from_le_bytes([sec.data[0], sec.data[1], sec.data[2], sec.data[3]]);
        assert_eq!(w0 & 0x7f, 0x6f);
        assert_ne!(w0 & 0xFFFFF000, 0);
    }

    #[test]
    fn elf_object_has_riscv_machine_type() {
        let mut mf = MachineFunction::new("f".into());
        let b = mf.add_block("entry");
        mf.push(b, MInstr::new(NOP));
        let mut e = RiscVEmitter::new(ObjectFormat::Elf);
        let obj = emit_object(&mf, &mut e);
        let bytes = obj.to_bytes();
        let e_machine = u16::from_le_bytes([bytes[18], bytes[19]]);
        assert_eq!(e_machine, 243, "EM_RISCV");
    }

    #[test]
    #[should_panic(expected = "unsupported RISC-V opcode")]
    fn enc_unknown_opcode_panics() {
        let _ = encode_instr(&MInstr::new(llvm_codegen::isel::MOpcode(0xFFFF)));
    }

    // ── A-extension encoding tests ─────────────────────────────────────────

    #[test]
    fn enc_fence_iorw() {
        let mi = MInstr::new(FENCE);
        assert_eq!(encode_instr(&mi), 0x0FF0_000F);
    }

    #[test]
    fn enc_amo_amoadd_w() {
        // amoadd.w.aqrl rd=x1, rs2=x2, (rs1=x3)
        let mi = MInstr::new(AMOADD_W)
            .with_dst(VReg(1))
            .with_preg(PReg(3))
            .with_preg(PReg(2));
        let expected = enc_amo(0b00000, true, true, 2, 3, 0b010, 1);
        assert_eq!(encode_instr(&mi), expected, "amoadd.w encoding mismatch");
    }

    #[test]
    fn enc_amo_lr_w() {
        // lr.w.aqrl rd=x5, (rs1=x1); rs2 must be 0
        let mi = MInstr::new(LR_W).with_dst(VReg(5)).with_preg(PReg(1));
        let expected = enc_amo(0b00010, true, true, 0, 1, 0b010, 5);
        assert_eq!(encode_instr(&mi), expected);
    }

    #[test]
    fn enc_amo_sc_w() {
        // sc.w.aqrl rd=x1, rs2=x2, (rs1=x3)
        let mi = MInstr::new(SC_W)
            .with_dst(VReg(1))
            .with_preg(PReg(3))
            .with_preg(PReg(2));
        let expected = enc_amo(0b00011, true, true, 2, 3, 0b010, 1);
        assert_eq!(encode_instr(&mi), expected);
    }

    #[test]
    fn enc_amo_amoswap_d() {
        // amoswap.d.aqrl rd=x1, rs2=x3, (rs1=x2) — 64-bit variant
        let mi = MInstr::new(AMOSWAP_D)
            .with_dst(VReg(1))
            .with_preg(PReg(2))
            .with_preg(PReg(3));
        let expected = enc_amo(0b00001, true, true, 3, 2, 0b011, 1);
        assert_eq!(encode_instr(&mi), expected);
    }

    #[test]
    fn enc_amo_amoxor_w() {
        let mi = MInstr::new(AMOXOR_W)
            .with_dst(VReg(2))
            .with_preg(PReg(4))
            .with_preg(PReg(5));
        let expected = enc_amo(0b00100, true, true, 5, 4, 0b010, 2);
        assert_eq!(encode_instr(&mi), expected);
    }

    #[test]
    fn enc_amo_amoand_w() {
        let mi = MInstr::new(AMOAND_W)
            .with_dst(VReg(3))
            .with_preg(PReg(1))
            .with_preg(PReg(2));
        let expected = enc_amo(0b01100, true, true, 2, 1, 0b010, 3);
        assert_eq!(encode_instr(&mi), expected);
    }

    #[test]
    fn enc_amo_amoor_w() {
        let mi = MInstr::new(AMOOR_W)
            .with_dst(VReg(4))
            .with_preg(PReg(5))
            .with_preg(PReg(6));
        let expected = enc_amo(0b01000, true, true, 6, 5, 0b010, 4);
        assert_eq!(encode_instr(&mi), expected);
    }

    #[test]
    fn enc_amo_amomin_w() {
        let mi = MInstr::new(AMOMIN_W)
            .with_dst(VReg(1))
            .with_preg(PReg(2))
            .with_preg(PReg(3));
        let expected = enc_amo(0b10000, true, true, 3, 2, 0b010, 1);
        assert_eq!(encode_instr(&mi), expected);
    }

    #[test]
    fn enc_amo_amomax_w() {
        let mi = MInstr::new(AMOMAX_W)
            .with_dst(VReg(1))
            .with_preg(PReg(2))
            .with_preg(PReg(3));
        let expected = enc_amo(0b10100, true, true, 3, 2, 0b010, 1);
        assert_eq!(encode_instr(&mi), expected);
    }

    #[test]
    fn enc_amo_amominu_w() {
        let mi = MInstr::new(AMOMINU_W)
            .with_dst(VReg(1))
            .with_preg(PReg(2))
            .with_preg(PReg(3));
        let expected = enc_amo(0b11000, true, true, 3, 2, 0b010, 1);
        assert_eq!(encode_instr(&mi), expected);
    }

    #[test]
    fn enc_amo_amomaxu_w() {
        let mi = MInstr::new(AMOMAXU_W)
            .with_dst(VReg(1))
            .with_preg(PReg(2))
            .with_preg(PReg(3));
        let expected = enc_amo(0b11100, true, true, 3, 2, 0b010, 1);
        assert_eq!(encode_instr(&mi), expected);
    }

    #[test]
    fn enc_amo_lr_d() {
        let mi = MInstr::new(LR_D).with_dst(VReg(5)).with_preg(PReg(1));
        let expected = enc_amo(0b00010, true, true, 0, 1, 0b011, 5);
        assert_eq!(encode_instr(&mi), expected);
    }

    #[test]
    fn enc_amo_sc_d() {
        let mi = MInstr::new(SC_D)
            .with_dst(VReg(1))
            .with_preg(PReg(3))
            .with_preg(PReg(2));
        let expected = enc_amo(0b00011, true, true, 2, 3, 0b011, 1);
        assert_eq!(encode_instr(&mi), expected);
    }

    #[test]
    fn enc_amo_opcode_field() {
        // All AMO instructions must have opcode bits [6:0] = 0x2F
        let opcodes = [
            FENCE, LR_W, SC_W, AMOADD_W, AMOSWAP_W, AMOXOR_W, AMOAND_W, AMOOR_W, AMOMIN_W,
            AMOMAX_W, AMOMINU_W, AMOMAXU_W, LR_D, SC_D, AMOADD_D, AMOSWAP_D, AMOXOR_D, AMOAND_D,
            AMOOR_D,
        ];
        // FENCE is not an AMO instruction; skip it in this check
        let amo_opcodes = &opcodes[1..]; // skip FENCE
        for &op in amo_opcodes {
            let mi = MInstr::new(op)
                .with_dst(VReg(1))
                .with_preg(PReg(2))
                .with_preg(PReg(3));
            let word = encode_instr(&mi);
            assert_eq!(
                word & 0x7F,
                0x2F,
                "opcode 0x{:X} should have AMO opcode 0x2F in bits[6:0], got 0x{:X}",
                op.0,
                word & 0x7F
            );
        }
    }

    // ── F+D extension encoding tests ───────────────────────────────────────

    /// Build an FP R-type instruction with dst=PReg(fd), rs1=PReg(fs1), rs2=PReg(fs2).
    /// FP PRegs are offset by 32: PReg(32+fd), etc.
    fn fp_rr(op: llvm_codegen::isel::MOpcode, fd: u8, fs1: u8, fs2: u8) -> MInstr {
        MInstr::new(op)
            .with_dst(VReg((32 + fd) as u32)) // post-regalloc: VReg holds PReg number
            .with_preg(PReg(32 + fs1))
            .with_preg(PReg(32 + fs2))
    }

    /// Build a unary FP instruction (one source, one destination).
    fn fp_r1(op: llvm_codegen::isel::MOpcode, fd: u8, fs1: u8) -> MInstr {
        MInstr::new(op)
            .with_dst(VReg((32 + fd) as u32))
            .with_preg(PReg(32 + fs1))
    }

    #[test]
    fn enc_fadd_d_opcode_field() {
        // fadd.d f3, f1, f2: funct7=0x01, funct3=0x7 (RNE), opcode=0x53
        let mi = fp_rr(FADD_D, 3, 1, 2);
        let word = encode_instr(&mi);
        assert_eq!(word & 0x7F, 0x53, "fadd.d opcode must be 0x53");
        assert_eq!((word >> 25) & 0x7F, 0x01, "fadd.d funct7 must be 0x01");
        assert_eq!((word >> 12) & 0x7, 0x7, "fadd.d funct3 must be 0x7 (RNE)");
        assert_eq!(word, enc_r(0x01, 2, 1, 0x7, 3, 0x53));
    }

    #[test]
    fn enc_fsub_d_opcode_field() {
        let mi = fp_rr(FSUB_D, 4, 5, 6);
        let word = encode_instr(&mi);
        assert_eq!(word & 0x7F, 0x53, "fsub.d opcode must be 0x53");
        assert_eq!((word >> 25) & 0x7F, 0x05, "fsub.d funct7 must be 0x05");
        assert_eq!(word, enc_r(0x05, 6, 5, 0x7, 4, 0x53));
    }

    #[test]
    fn enc_fmul_d_opcode_field() {
        let mi = fp_rr(FMUL_D, 0, 1, 2);
        let word = encode_instr(&mi);
        assert_eq!((word >> 25) & 0x7F, 0x09, "fmul.d funct7 must be 0x09");
        assert_eq!(word, enc_r(0x09, 2, 1, 0x7, 0, 0x53));
    }

    #[test]
    fn enc_fdiv_d_opcode_field() {
        let mi = fp_rr(FDIV_D, 0, 1, 2);
        let word = encode_instr(&mi);
        assert_eq!((word >> 25) & 0x7F, 0x0D, "fdiv.d funct7 must be 0x0D");
        assert_eq!(word, enc_r(0x0D, 2, 1, 0x7, 0, 0x53));
    }

    #[test]
    fn enc_fsqrt_d_opcode_field() {
        // fsqrt.d f3, f1: rs2 must be 0
        let mi = fp_r1(FSQRT_D, 3, 1);
        let word = encode_instr(&mi);
        assert_eq!(word & 0x7F, 0x53, "fsqrt.d opcode");
        assert_eq!((word >> 25) & 0x7F, 0x2D, "fsqrt.d funct7 must be 0x2D");
        assert_eq!((word >> 20) & 0x1F, 0, "fsqrt.d rs2 must be 0");
        assert_eq!(word, enc_r(0x2D, 0, 1, 0x7, 3, 0x53));
    }

    #[test]
    fn enc_fneg_d_uses_fsgnjn_encoding() {
        // fneg.d is FSGNJN.D rd, rs, rs: funct7=0x21, funct3=0x1, rs2=rs1
        let mi = MInstr::new(FNEG_D)
            .with_dst(VReg(32 + 0)) // fd=f0
            .with_preg(PReg(32 + 1)) // rs1=f1
            .with_preg(PReg(32 + 1)); // rs2=f1 (same)
        let word = encode_instr(&mi);
        assert_eq!(
            (word >> 25) & 0x7F,
            0x21,
            "fneg.d funct7 must be 0x21 (FSGNJN)"
        );
        assert_eq!((word >> 12) & 0x7, 0x1, "fneg.d funct3 must be 0x1");
        // rs1 == rs2 == 1
        assert_eq!((word >> 15) & 0x1F, 1, "rs1 must be f1");
        assert_eq!((word >> 20) & 0x1F, 1, "rs2 must be f1 (same as rs1)");
    }

    #[test]
    fn enc_feq_d_produces_integer_dest() {
        // feq.d uses integer rd, FP rs1/rs2: funct7=0x51, funct3=0x2
        let mi = MInstr::new(FCMP_EQ_D)
            .with_dst(VReg(3)) // integer rd=x3
            .with_preg(PReg(32 + 0)) // rs1=f0
            .with_preg(PReg(32 + 1)); // rs2=f1
        let word = encode_instr(&mi);
        assert_eq!((word >> 25) & 0x7F, 0x51, "feq.d funct7 must be 0x51");
        assert_eq!((word >> 12) & 0x7, 0x2, "feq.d funct3 must be 0x2");
        assert_eq!((word >> 7) & 0x1F, 3, "feq.d integer rd must be 3");
        assert_eq!(word, enc_r(0x51, 1, 0, 0x2, 3, 0x53));
    }

    #[test]
    fn enc_flt_d_opcode_field() {
        let mi = MInstr::new(FCMP_LT_D)
            .with_dst(VReg(1))
            .with_preg(PReg(32 + 2))
            .with_preg(PReg(32 + 3));
        let word = encode_instr(&mi);
        assert_eq!((word >> 25) & 0x7F, 0x51, "flt.d funct7");
        assert_eq!((word >> 12) & 0x7, 0x1, "flt.d funct3 must be 0x1");
        assert_eq!(word, enc_r(0x51, 3, 2, 0x1, 1, 0x53));
    }

    #[test]
    fn enc_fle_d_opcode_field() {
        let mi = MInstr::new(FCMP_LE_D)
            .with_dst(VReg(1))
            .with_preg(PReg(32 + 4))
            .with_preg(PReg(32 + 5));
        let word = encode_instr(&mi);
        assert_eq!((word >> 12) & 0x7, 0x0, "fle.d funct3 must be 0x0");
        assert_eq!(word, enc_r(0x51, 5, 4, 0x0, 1, 0x53));
    }

    #[test]
    fn enc_fcvt_l_d_funct7_and_rs2() {
        // fcvt.l.d: funct7=0x61, rs2=2 (L), rounding=RTZ=0x1
        let mi = MInstr::new(FCVT_L_D)
            .with_dst(VReg(1)) // integer rd
            .with_preg(PReg(32 + 0)); // fp rs1
        let word = encode_instr(&mi);
        assert_eq!((word >> 25) & 0x7F, 0x61, "fcvt.l.d funct7 must be 0x61");
        assert_eq!((word >> 20) & 0x1F, 2, "fcvt.l.d rs2 must be 2 (L)");
        assert_eq!((word >> 12) & 0x7, 0x1, "fcvt.l.d rounding must be RTZ=0x1");
    }

    #[test]
    fn enc_fcvt_w_d_funct7_and_rs2() {
        // fcvt.w.d: funct7=0x61, rs2=0 (W), rounding=RTZ=0x1
        let mi = MInstr::new(FCVT_W_D)
            .with_dst(VReg(2))
            .with_preg(PReg(32 + 1));
        let word = encode_instr(&mi);
        assert_eq!((word >> 25) & 0x7F, 0x61, "fcvt.w.d funct7");
        assert_eq!((word >> 20) & 0x1F, 0, "fcvt.w.d rs2 must be 0 (W)");
    }

    #[test]
    fn enc_fcvt_d_l_funct7_and_rs2() {
        // fcvt.d.l: funct7=0x69, rs2=2
        let mi = MInstr::new(FCVT_D_L)
            .with_dst(VReg(32 + 0)) // fp rd
            .with_preg(PReg(1)); // int rs1
        let word = encode_instr(&mi);
        assert_eq!((word >> 25) & 0x7F, 0x69, "fcvt.d.l funct7 must be 0x69");
        assert_eq!((word >> 20) & 0x1F, 2, "fcvt.d.l rs2 must be 2 (L)");
        assert_eq!(word & 0x7F, 0x53, "opcode must be 0x53");
    }

    #[test]
    fn enc_fcvt_d_w_funct7_and_rs2() {
        // fcvt.d.w: funct7=0x69, rs2=0
        let mi = MInstr::new(FCVT_D_W)
            .with_dst(VReg(32 + 2))
            .with_preg(PReg(3));
        let word = encode_instr(&mi);
        assert_eq!((word >> 25) & 0x7F, 0x69, "fcvt.d.w funct7 must be 0x69");
        assert_eq!((word >> 20) & 0x1F, 0, "fcvt.d.w rs2 must be 0 (W)");
    }

    #[test]
    fn enc_fmv_d_d_uses_fsgnj_encoding() {
        // fsgnj.d rd, rs, rs: funct7=0x11, funct3=0x0, rs2=rs1
        let mi = MInstr::new(FMV_D_D)
            .with_dst(VReg(32 + 5))
            .with_preg(PReg(32 + 6));
        let word = encode_instr(&mi);
        assert_eq!(
            (word >> 25) & 0x7F,
            0x11,
            "fmv.d funct7 must be 0x11 (FSGNJ)"
        );
        assert_eq!((word >> 12) & 0x7, 0x0, "fmv.d funct3 must be 0x0");
        // Both rs1 and rs2 must be the same source register (f6=6)
        assert_eq!((word >> 15) & 0x1F, 6, "fmv.d rs1 must be f6");
        assert_eq!((word >> 20) & 0x1F, 6, "fmv.d rs2 must be f6 (copy idiom)");
    }

    #[test]
    fn enc_fld_i_type_opcode_and_funct3() {
        // fld f1, 8(x2): opcode=0x07, funct3=0x3
        let mi = MInstr::new(FLD)
            .with_dst(VReg(32 + 1)) // fd=f1
            .with_preg(PReg(2)) // rs1=x2
            .with_imm(8);
        let word = encode_instr(&mi);
        assert_eq!(word & 0x7F, 0x07, "fld opcode must be 0x07");
        assert_eq!((word >> 12) & 0x7, 0x3, "fld funct3 must be 0x3");
        assert_eq!((word >> 20) & 0xFFF, 8, "fld imm must be 8");
        assert_eq!((word >> 7) & 0x1F, 1, "fld rd must be f1=1");
        assert_eq!((word >> 15) & 0x1F, 2, "fld rs1 must be x2");
        assert_eq!(word, enc_i(8, 2, 0x3, 1, 0x07));
    }

    #[test]
    fn enc_fsd_s_type_opcode_and_funct3() {
        // fsd f2, 16(x3): opcode=0x27, funct3=0x3
        let mi = MInstr::new(FSD)
            .with_preg(PReg(3)) // rs1=x3 (base)
            .with_preg(PReg(32 + 2)) // rs2=f2 (source)
            .with_imm(16);
        let word = encode_instr(&mi);
        assert_eq!(word & 0x7F, 0x27, "fsd opcode must be 0x27");
        assert_eq!((word >> 12) & 0x7, 0x3, "fsd funct3 must be 0x3");
        // Verify it matches enc_s
        assert_eq!(word, enc_s(16, 2, 3, 0x3, 0x27));
    }

    #[test]
    fn enc_fp_arith_all_have_opcode_53() {
        // All FP arithmetic instructions must have opcode 0x53.
        let fp_arith_ops = [
            FADD_D, FSUB_D, FMUL_D, FDIV_D, FSQRT_D, FNEG_D, FCMP_EQ_D, FCMP_LT_D, FCMP_LE_D,
            FADD_S, FSUB_S, FMUL_S, FDIV_S, FNEG_S, FCMP_EQ_S, FCMP_LT_S, FCMP_LE_S,
        ];
        for &op in &fp_arith_ops {
            // Build a generic instruction: for compare ops dst is int, for arith dst is fp.
            let mi = if matches!(
                op,
                FCMP_EQ_D | FCMP_LT_D | FCMP_LE_D | FCMP_EQ_S | FCMP_LT_S | FCMP_LE_S
            ) {
                MInstr::new(op)
                    .with_dst(VReg(1))
                    .with_preg(PReg(32))
                    .with_preg(PReg(33))
            } else if matches!(op, FSQRT_D) {
                MInstr::new(op).with_dst(VReg(32)).with_preg(PReg(33))
            } else if matches!(op, FNEG_D | FNEG_S) {
                MInstr::new(op)
                    .with_dst(VReg(32))
                    .with_preg(PReg(33))
                    .with_preg(PReg(33))
            } else {
                MInstr::new(op)
                    .with_dst(VReg(32))
                    .with_preg(PReg(33))
                    .with_preg(PReg(34))
            };
            let word = encode_instr(&mi);
            assert_eq!(word & 0x7F, 0x53, "FP op {:?} must have opcode 0x53", op);
        }
    }

    #[test]
    fn enc_fadd_s_funct7_is_zero() {
        // fadd.s: funct7=0x00
        let mi = fp_rr(FADD_S, 0, 1, 2);
        let word = encode_instr(&mi);
        assert_eq!((word >> 25) & 0x7F, 0x00, "fadd.s funct7 must be 0x00");
    }

    #[test]
    fn enc_fcvt_s_l_funct7_and_rs2() {
        // fcvt.s.l: funct7=0x68, rs2=2
        let mi = MInstr::new(FCVT_S_L)
            .with_dst(VReg(32 + 0))
            .with_preg(PReg(1));
        let word = encode_instr(&mi);
        assert_eq!((word >> 25) & 0x7F, 0x68, "fcvt.s.l funct7 must be 0x68");
        assert_eq!((word >> 20) & 0x1F, 2, "fcvt.s.l rs2 must be 2");
    }

    // ── IR-level lowering tests ────────────────────────────────────────────

    #[test]
    fn lower_fadd_double_emits_fadd_d() {
        use crate::lower::RiscVBackend;
        use llvm_codegen::isel::IselBackend;
        use llvm_ir_parser::parser::parse;
        let src = r#"define double @f(double %a, double %b) {
entry:
  %r = fadd double %a, %b
  ret double %r
}"#;
        let (ctx, module) = parse(src).expect("parse");
        let func = &module.functions[0];
        let mut be = RiscVBackend;
        let mf = be.lower_function(&ctx, &module, func);
        let has_fadd = mf
            .blocks
            .iter()
            .flat_map(|b| b.instrs.iter())
            .any(|i| i.opcode == FADD_D);
        assert!(has_fadd, "fadd double must lower to FADD_D");
    }

    #[test]
    fn lower_fsub_double_emits_fsub_d() {
        use crate::lower::RiscVBackend;
        use llvm_codegen::isel::IselBackend;
        use llvm_ir_parser::parser::parse;
        let src = r#"define double @f(double %a, double %b) {
entry:
  %r = fsub double %a, %b
  ret double %r
}"#;
        let (ctx, module) = parse(src).expect("parse");
        let func = &module.functions[0];
        let mut be = RiscVBackend;
        let mf = be.lower_function(&ctx, &module, func);
        let has = mf
            .blocks
            .iter()
            .flat_map(|b| b.instrs.iter())
            .any(|i| i.opcode == FSUB_D);
        assert!(has, "fsub double must lower to FSUB_D");
    }

    #[test]
    fn lower_fmul_double_emits_fmul_d() {
        use crate::lower::RiscVBackend;
        use llvm_codegen::isel::IselBackend;
        use llvm_ir_parser::parser::parse;
        let src = r#"define double @f(double %a, double %b) {
entry:
  %r = fmul double %a, %b
  ret double %r
}"#;
        let (ctx, module) = parse(src).expect("parse");
        let func = &module.functions[0];
        let mut be = RiscVBackend;
        let mf = be.lower_function(&ctx, &module, func);
        let has = mf
            .blocks
            .iter()
            .flat_map(|b| b.instrs.iter())
            .any(|i| i.opcode == FMUL_D);
        assert!(has, "fmul double must lower to FMUL_D");
    }

    #[test]
    fn lower_fdiv_double_emits_fdiv_d() {
        use crate::lower::RiscVBackend;
        use llvm_codegen::isel::IselBackend;
        use llvm_ir_parser::parser::parse;
        let src = r#"define double @f(double %a, double %b) {
entry:
  %r = fdiv double %a, %b
  ret double %r
}"#;
        let (ctx, module) = parse(src).expect("parse");
        let func = &module.functions[0];
        let mut be = RiscVBackend;
        let mf = be.lower_function(&ctx, &module, func);
        let has = mf
            .blocks
            .iter()
            .flat_map(|b| b.instrs.iter())
            .any(|i| i.opcode == FDIV_D);
        assert!(has, "fdiv double must lower to FDIV_D");
    }

    #[test]
    fn lower_fneg_double_emits_fneg_d() {
        use crate::lower::RiscVBackend;
        use llvm_codegen::isel::IselBackend;
        use llvm_ir_parser::parser::parse;
        let src = r#"define double @f(double %a) {
entry:
  %r = fneg double %a
  ret double %r
}"#;
        let (ctx, module) = parse(src).expect("parse");
        let func = &module.functions[0];
        let mut be = RiscVBackend;
        let mf = be.lower_function(&ctx, &module, func);
        let has = mf
            .blocks
            .iter()
            .flat_map(|b| b.instrs.iter())
            .any(|i| i.opcode == FNEG_D);
        assert!(has, "fneg double must lower to FNEG_D");
    }

    #[test]
    fn lower_fcmp_oeq_double_emits_fcmp_eq_d() {
        use crate::lower::RiscVBackend;
        use llvm_codegen::isel::IselBackend;
        use llvm_ir_parser::parser::parse;
        let src = r#"define i1 @f(double %a, double %b) {
entry:
  %r = fcmp oeq double %a, %b
  ret i1 %r
}"#;
        let (ctx, module) = parse(src).expect("parse");
        let func = &module.functions[0];
        let mut be = RiscVBackend;
        let mf = be.lower_function(&ctx, &module, func);
        let has = mf
            .blocks
            .iter()
            .flat_map(|b| b.instrs.iter())
            .any(|i| i.opcode == FCMP_EQ_D);
        assert!(has, "fcmp oeq double must lower to FCMP_EQ_D");
    }

    #[test]
    fn lower_fcmp_olt_double_emits_fcmp_lt_d() {
        use crate::lower::RiscVBackend;
        use llvm_codegen::isel::IselBackend;
        use llvm_ir_parser::parser::parse;
        let src = r#"define i1 @f(double %a, double %b) {
entry:
  %r = fcmp olt double %a, %b
  ret i1 %r
}"#;
        let (ctx, module) = parse(src).expect("parse");
        let func = &module.functions[0];
        let mut be = RiscVBackend;
        let mf = be.lower_function(&ctx, &module, func);
        let has = mf
            .blocks
            .iter()
            .flat_map(|b| b.instrs.iter())
            .any(|i| i.opcode == FCMP_LT_D);
        assert!(has, "fcmp olt double must lower to FCMP_LT_D");
    }

    #[test]
    fn lower_sitofp_emits_fcvt_d_l() {
        use crate::lower::RiscVBackend;
        use llvm_codegen::isel::IselBackend;
        use llvm_ir_parser::parser::parse;
        let src = r#"define double @f(i64 %a) {
entry:
  %r = sitofp i64 %a to double
  ret double %r
}"#;
        let (ctx, module) = parse(src).expect("parse");
        let func = &module.functions[0];
        let mut be = RiscVBackend;
        let mf = be.lower_function(&ctx, &module, func);
        let has = mf
            .blocks
            .iter()
            .flat_map(|b| b.instrs.iter())
            .any(|i| i.opcode == FCVT_D_L);
        assert!(has, "sitofp i64 to double must lower to FCVT_D_L");
    }

    #[test]
    fn lower_fptosi_emits_fcvt_l_d() {
        use crate::lower::RiscVBackend;
        use llvm_codegen::isel::IselBackend;
        use llvm_ir_parser::parser::parse;
        let src = r#"define i64 @f(double %a) {
entry:
  %r = fptosi double %a to i64
  ret i64 %r
}"#;
        let (ctx, module) = parse(src).expect("parse");
        let func = &module.functions[0];
        let mut be = RiscVBackend;
        let mf = be.lower_function(&ctx, &module, func);
        let has = mf
            .blocks
            .iter()
            .flat_map(|b| b.instrs.iter())
            .any(|i| i.opcode == FCVT_L_D);
        assert!(has, "fptosi double to i64 must lower to FCVT_L_D");
    }

    #[test]
    fn lower_fadd_float_emits_fadd_s() {
        use crate::lower::RiscVBackend;
        use llvm_codegen::isel::IselBackend;
        use llvm_ir_parser::parser::parse;
        let src = r#"define float @f(float %a, float %b) {
entry:
  %r = fadd float %a, %b
  ret float %r
}"#;
        let (ctx, module) = parse(src).expect("parse");
        let func = &module.functions[0];
        let mut be = RiscVBackend;
        let mf = be.lower_function(&ctx, &module, func);
        let has = mf
            .blocks
            .iter()
            .flat_map(|b| b.instrs.iter())
            .any(|i| i.opcode == FADD_S);
        assert!(has, "fadd float must lower to FADD_S");
    }

    #[test]
    fn fp_vreg_allocation_uses_fp_pool() {
        // Verify that after lowering a double function, float VRegs are allocated
        // to FP PRegs (PReg 32-63), not integer PRegs.
        use crate::lower::RiscVBackend;
        use llvm_codegen::{
            isel::IselBackend,
            regalloc::{allocate_registers, compute_live_intervals, RegAllocStrategy},
        };
        use llvm_ir_parser::parser::parse;
        let src = r#"define double @f(double %a, double %b) {
entry:
  %r = fadd double %a, %b
  ret double %r
}"#;
        let (ctx, module) = parse(src).expect("parse");
        let func = &module.functions[0];
        let mut be = RiscVBackend;
        let mf = be.lower_function(&ctx, &module, func);

        let intervals = compute_live_intervals(&mf);
        let result = allocate_registers(
            &intervals,
            &mf.allocatable_pregs,
            &mf.allocatable_fp_pregs,
            RegAllocStrategy::LinearScan,
        );

        // Any Float-class VRegs must be assigned to the FP pool (PReg >= 32).
        use llvm_codegen::isel::RegClass;
        for (vr, &pr) in &result.vreg_to_preg {
            if vr.class() == RegClass::Float {
                assert!(
                    pr.0 >= 32,
                    "float VReg {:?} was assigned to int PReg {:?}",
                    vr,
                    pr
                );
            }
        }
    }
}
