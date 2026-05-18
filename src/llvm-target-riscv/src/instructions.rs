//! RISC-V machine opcode constants used by lowering and encoder.

use llvm_codegen::isel::MOpcode;

/// Public API for `NOP`.
pub const NOP: MOpcode = MOpcode(0x00);
/// Public API for `MOV_RR`.
pub const MOV_RR: MOpcode = MOpcode(0x01);
/// Public API for `MOV_IMM`.
pub const MOV_IMM: MOpcode = MOpcode(0x02);
/// Public API for `MOV_PR`.
pub const MOV_PR: MOpcode = MOpcode(0x03);

/// Public API for `ADD_RR`.
pub const ADD_RR: MOpcode = MOpcode(0x10);
/// Public API for `SUB_RR`.
pub const SUB_RR: MOpcode = MOpcode(0x11);
/// Public API for `MUL_RR`.
pub const MUL_RR: MOpcode = MOpcode(0x12);
/// Public API for `DIV_RR`.
pub const DIV_RR: MOpcode = MOpcode(0x13);
/// Public API for `UDIV_RR`.
pub const UDIV_RR: MOpcode = MOpcode(0x14);
/// Public API for `REM_RR`.
pub const REM_RR: MOpcode = MOpcode(0x15);
/// Public API for `UREM_RR`.
pub const UREM_RR: MOpcode = MOpcode(0x16);

/// Public API for `AND_RR`.
pub const AND_RR: MOpcode = MOpcode(0x20);
/// Public API for `OR_RR`.
pub const OR_RR: MOpcode = MOpcode(0x21);
/// Public API for `XOR_RR`.
pub const XOR_RR: MOpcode = MOpcode(0x22);
/// Public API for `SLL_RR`.
pub const SLL_RR: MOpcode = MOpcode(0x23);
/// Public API for `SRL_RR`.
pub const SRL_RR: MOpcode = MOpcode(0x24);
/// Public API for `SRA_RR`.
pub const SRA_RR: MOpcode = MOpcode(0x25);
/// Public API for `SLT_RR`.
pub const SLT_RR: MOpcode = MOpcode(0x26);
/// Public API for `SLTU_RR`.
pub const SLTU_RR: MOpcode = MOpcode(0x27);

/// Public API for `ADDI`.
pub const ADDI: MOpcode = MOpcode(0x30);
/// Public API for `XORI`.
pub const XORI: MOpcode = MOpcode(0x31);
/// Public API for `SLTIU`.
pub const SLTIU: MOpcode = MOpcode(0x32);

/// Public API for `LW`.
pub const LW: MOpcode = MOpcode(0x40);
/// Public API for `LD`.
pub const LD: MOpcode = MOpcode(0x41);
/// Public API for `SW`.
pub const SW: MOpcode = MOpcode(0x42);
/// Public API for `SD`.
pub const SD: MOpcode = MOpcode(0x43);

/// Public API for `BEQ`.
pub const BEQ: MOpcode = MOpcode(0x50);
/// Public API for `BNE`.
pub const BNE: MOpcode = MOpcode(0x51);
/// Public API for `BLT`.
pub const BLT: MOpcode = MOpcode(0x52);
/// Public API for `BGE`.
pub const BGE: MOpcode = MOpcode(0x53);
/// Public API for `BLTU`.
pub const BLTU: MOpcode = MOpcode(0x54);
/// Public API for `BGEU`.
pub const BGEU: MOpcode = MOpcode(0x55);

/// Public API for `JAL`.
pub const JAL: MOpcode = MOpcode(0x60);
/// Public API for `JALR`.
pub const JALR: MOpcode = MOpcode(0x61);
/// Public API for `RET`.
pub const RET: MOpcode = MOpcode(0x62);

/// Public API for `LUI`.
pub const LUI: MOpcode = MOpcode(0x70);
/// Public API for `AUIPC`.
pub const AUIPC: MOpcode = MOpcode(0x71);

// ── A-extension atomics ────────────────────────────────────────────────────

/// `fence iorw,iorw` (sequential consistency fence).
pub const FENCE: MOpcode = MOpcode(0x80);
/// `lr.w rd, (rs1)` — load-reserved word.
pub const LR_W: MOpcode = MOpcode(0x81);
/// `sc.w rd, rs2, (rs1)` — store-conditional word.
pub const SC_W: MOpcode = MOpcode(0x82);
/// `amoadd.w rd, rs2, (rs1)`.
pub const AMOADD_W: MOpcode = MOpcode(0x83);
/// `amoswap.w rd, rs2, (rs1)`.
pub const AMOSWAP_W: MOpcode = MOpcode(0x84);
/// `amoxor.w rd, rs2, (rs1)`.
pub const AMOXOR_W: MOpcode = MOpcode(0x85);
/// `amoand.w rd, rs2, (rs1)`.
pub const AMOAND_W: MOpcode = MOpcode(0x86);
/// `amoor.w rd, rs2, (rs1)`.
pub const AMOOR_W: MOpcode = MOpcode(0x87);
/// `amomin.w rd, rs2, (rs1)`.
pub const AMOMIN_W: MOpcode = MOpcode(0x88);
/// `amomax.w rd, rs2, (rs1)`.
pub const AMOMAX_W: MOpcode = MOpcode(0x89);
/// `amominu.w rd, rs2, (rs1)`.
pub const AMOMINU_W: MOpcode = MOpcode(0x8A);
/// `amomaxu.w rd, rs2, (rs1)`.
pub const AMOMAXU_W: MOpcode = MOpcode(0x8B);

// 64-bit (D) variants for RV64.
/// `lr.d rd, (rs1)` — load-reserved doubleword.
pub const LR_D: MOpcode = MOpcode(0x8C);
/// `sc.d rd, rs2, (rs1)` — store-conditional doubleword.
pub const SC_D: MOpcode = MOpcode(0x8D);
/// `amoadd.d rd, rs2, (rs1)`.
pub const AMOADD_D: MOpcode = MOpcode(0x8E);
/// `amoswap.d rd, rs2, (rs1)`.
pub const AMOSWAP_D: MOpcode = MOpcode(0x8F);
/// `amoxor.d rd, rs2, (rs1)`.
pub const AMOXOR_D: MOpcode = MOpcode(0x90);
/// `amoand.d rd, rs2, (rs1)`.
pub const AMOAND_D: MOpcode = MOpcode(0x91);
/// `amoor.d rd, rs2, (rs1)`.
pub const AMOOR_D: MOpcode = MOpcode(0x92);

// ── non-promotable alloca frame-slot access ────────────────────────────────
/// `addi rd, s0, -(slot_idx+1)*8`  — materialize a non-promotable alloca address.
/// s0 (x8) is the RISC-V frame pointer. `dst` = VReg, `operands[0]` = `Imm(slot_idx)`.
pub const ADDI_FP_SLOT: MOpcode = MOpcode(0xA0);
/// `ld rd, 0(rs1)` — 64-bit load through a register-held pointer (offset 0).
/// `dst` = VReg, `operands[0]` = PReg (pointer register after regalloc).
pub const LD_REG: MOpcode = MOpcode(0xA1);
/// `sd rs2, 0(rs1)` — 64-bit store through a register-held pointer (offset 0).
/// No `dst`. `operands[0]` = PReg (pointer), `operands[1]` = PReg (value).
pub const SD_REG: MOpcode = MOpcode(0xA2);

// ── F+D extension (scalar FP) ─────────────────────────────────────────────

/// `fadd.d rd, rs1, rs2` — double-precision add (funct7=0x01, funct3=RNE=0x7).
pub const FADD_D: MOpcode = MOpcode(0xB0);
/// `fsub.d rd, rs1, rs2` — double-precision subtract (funct7=0x05).
pub const FSUB_D: MOpcode = MOpcode(0xB1);
/// `fmul.d rd, rs1, rs2` — double-precision multiply (funct7=0x09).
pub const FMUL_D: MOpcode = MOpcode(0xB2);
/// `fdiv.d rd, rs1, rs2` — double-precision divide (funct7=0x0D).
pub const FDIV_D: MOpcode = MOpcode(0xB3);
/// `fsqrt.d rd, rs1` — double-precision square root (funct7=0x2D, rs2=0).
pub const FSQRT_D: MOpcode = MOpcode(0xB4);
/// `fsgnjn.d rd, rs1, rs2` — negate sign (used for FNeg; funct7=0x21, funct3=0x1, rs2=rs1).
pub const FNEG_D: MOpcode = MOpcode(0xB5);
/// `feq.d rd, rs1, rs2` — double FP equal comparison → integer result (funct7=0x51, funct3=0x2).
pub const FCMP_EQ_D: MOpcode = MOpcode(0xB6);
/// `flt.d rd, rs1, rs2` — double FP less-than comparison → integer result (funct7=0x51, funct3=0x1).
pub const FCMP_LT_D: MOpcode = MOpcode(0xB7);
/// `fle.d rd, rs1, rs2` — double FP less-equal comparison → integer result (funct7=0x51, funct3=0x0).
pub const FCMP_LE_D: MOpcode = MOpcode(0xB8);
/// `fcvt.w.d rd, rs1` — double→i32 conversion (funct7=0x61, rs2=0, rounding=RTZ=0x1).
pub const FCVT_W_D: MOpcode = MOpcode(0xB9);
/// `fcvt.l.d rd, rs1` — double→i64 conversion (funct7=0x61, rs2=2, rounding=RTZ=0x1).
pub const FCVT_L_D: MOpcode = MOpcode(0xBA);
/// `fcvt.d.w rd, rs1` — i32→double conversion (funct7=0x69, rs2=0).
pub const FCVT_D_W: MOpcode = MOpcode(0xBB);
/// `fcvt.d.l rd, rs1` — i64→double conversion (funct7=0x69, rs2=2).
pub const FCVT_D_L: MOpcode = MOpcode(0xBC);
/// `fld rd, imm(rs1)` — load double from memory (opcode=0x07, funct3=0x3).
pub const FLD: MOpcode = MOpcode(0xBD);
/// `fsd rs2, imm(rs1)` — store double to memory (opcode=0x27, funct3=0x3).
pub const FSD: MOpcode = MOpcode(0xBE);
/// `fsgnj.d rd, rs1, rs1` — FP register copy via sign-inject (funct7=0x11, funct3=0x0, rs2=rs1).
pub const FMV_D_D: MOpcode = MOpcode(0xBF);

// ── F+D spill opcodes (re-use FLD/FSD with frame-slot semantics) ───────────

/// FP spill load from stack slot (same encoding as FLD; semantics: dst = mem[fp + slot*8]).
pub const FP_LOAD_MR: MOpcode = MOpcode(0xC0);
/// FP spill store to stack slot (same encoding as FSD; semantics: mem[fp + slot*8] = src).
pub const FP_STORE_RM: MOpcode = MOpcode(0xC1);

// ── Single-precision (F extension) ────────────────────────────────────────

/// `fadd.s rd, rs1, rs2` — single-precision add (funct7=0x00).
pub const FADD_S: MOpcode = MOpcode(0xD0);
/// `fsub.s rd, rs1, rs2` — single-precision subtract (funct7=0x04).
pub const FSUB_S: MOpcode = MOpcode(0xD1);
/// `fmul.s rd, rs1, rs2` — single-precision multiply (funct7=0x08).
pub const FMUL_S: MOpcode = MOpcode(0xD2);
/// `fdiv.s rd, rs1, rs2` — single-precision divide (funct7=0x0C).
pub const FDIV_S: MOpcode = MOpcode(0xD3);
/// `fsgnjn.s rd, rs1, rs2` — negate sign single (funct7=0x20, funct3=0x1, rs2=rs1).
pub const FNEG_S: MOpcode = MOpcode(0xD4);
/// `feq.s rd, rs1, rs2` — single FP equal (funct7=0x50, funct3=0x2).
pub const FCMP_EQ_S: MOpcode = MOpcode(0xD5);
/// `flt.s rd, rs1, rs2` — single FP less-than (funct7=0x50, funct3=0x1).
pub const FCMP_LT_S: MOpcode = MOpcode(0xD6);
/// `fle.s rd, rs1, rs2` — single FP less-equal (funct7=0x50, funct3=0x0).
pub const FCMP_LE_S: MOpcode = MOpcode(0xD7);
/// `fcvt.w.s rd, rs1` — single→i32 (funct7=0x60, rs2=0, rounding=RTZ).
pub const FCVT_W_S: MOpcode = MOpcode(0xD8);
/// `fcvt.l.s rd, rs1` — single→i64 (funct7=0x60, rs2=2, rounding=RTZ).
pub const FCVT_L_S: MOpcode = MOpcode(0xD9);
/// `fcvt.s.w rd, rs1` — i32→single (funct7=0x68, rs2=0).
pub const FCVT_S_W: MOpcode = MOpcode(0xDA);
/// `fcvt.s.l rd, rs1` — i64→single (funct7=0x68, rs2=2).
pub const FCVT_S_L: MOpcode = MOpcode(0xDB);
/// `flw rd, imm(rs1)` — load single from memory (opcode=0x07, funct3=0x2).
pub const FLW: MOpcode = MOpcode(0xDC);
/// `fsw rs2, imm(rs1)` — store single to memory (opcode=0x27, funct3=0x2).
pub const FSW: MOpcode = MOpcode(0xDD);
