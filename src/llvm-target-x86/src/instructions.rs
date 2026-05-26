//! x86_64 machine opcode constants and condition-code values.

use llvm_codegen::isel::MOpcode;

// ── data movement ──────────────────────────────────────────────────────────
/// `mov dst, src`   (64-bit reg → reg)
pub const MOV_RR: MOpcode = MOpcode(0x00);
/// `mov dst, imm64` (64-bit immediate)
pub const MOV_RI: MOpcode = MOpcode(0x01);
/// Sign-extend 32-bit source to 64-bit destination (`movsxd`)
pub const MOVSX_32: MOpcode = MOpcode(0x02);
/// Sign-extend 8-bit source to 64-bit destination (`movsx`)
pub const MOVSX_8: MOpcode = MOpcode(0x03);
/// Zero-extend 8-bit source to 64-bit destination (`movzx`)
pub const MOVZX_8: MOpcode = MOpcode(0x04);
/// Sign-extend 16-bit source to 64-bit destination (`movsx`)
pub const MOVSX_16: MOpcode = MOpcode(0x06);
/// Move VReg source into a fixed physical register destination.
/// Layout: `operands[0]` = `PReg` (destination, ABI-fixed), `operands[1]` = `VReg`/`PReg` (source).
/// Used by `emit_mov_to_preg`; unlike `MOV_RR` there is no `dst` field so the
/// physical register in `operands[0]` survives register allocation unchanged.
pub const MOV_PR: MOpcode = MOpcode(0x05);

// ── integer arithmetic ─────────────────────────────────────────────────────
/// Public API for `ADD_RR`.
pub const ADD_RR: MOpcode = MOpcode(0x10);
/// Public API for `ADD_RI`.
pub const ADD_RI: MOpcode = MOpcode(0x11);
/// Public API for `SUB_RR`.
pub const SUB_RR: MOpcode = MOpcode(0x12);
/// Public API for `SUB_RI`.
pub const SUB_RI: MOpcode = MOpcode(0x13);
/// `imul dst, src`  (2-operand: dst *= src)
pub const IMUL_RR: MOpcode = MOpcode(0x14);
/// `imul dst, src, imm`  (3-operand)
pub const IMUL_RRI: MOpcode = MOpcode(0x15);
/// `idiv src`  (signed: rdx:rax ÷ src → rax=quot, rdx=rem; requires CQO first)
pub const IDIV_R: MOpcode = MOpcode(0x16);
/// `neg dst`
pub const NEG_R: MOpcode = MOpcode(0x17);
/// `cqo` — sign-extend rax into rdx:rax before idiv
pub const CQO: MOpcode = MOpcode(0x18);
/// `div src`  (unsigned: rdx:rax ÷ src → rax=quot, rdx=rem; requires `xor rdx, rdx` first)
pub const DIV_R: MOpcode = MOpcode(0x19);

// ── bitwise ────────────────────────────────────────────────────────────────
/// Public API for `AND_RR`.
pub const AND_RR: MOpcode = MOpcode(0x20);
/// Public API for `AND_RI`.
pub const AND_RI: MOpcode = MOpcode(0x21);
/// Public API for `OR_RR`.
pub const OR_RR: MOpcode = MOpcode(0x22);
/// Public API for `OR_RI`.
pub const OR_RI: MOpcode = MOpcode(0x23);
/// Public API for `XOR_RR`.
pub const XOR_RR: MOpcode = MOpcode(0x24);
/// Public API for `XOR_RI`.
pub const XOR_RI: MOpcode = MOpcode(0x25);
/// Public API for `NOT_R`.
pub const NOT_R: MOpcode = MOpcode(0x26);

// ── shifts ─────────────────────────────────────────────────────────────────
/// `shl dst, cl`  (logical left shift by register)
pub const SHL_RR: MOpcode = MOpcode(0x30);
/// `shl dst, imm8`
pub const SHL_RI: MOpcode = MOpcode(0x31);
/// `shr dst, cl`  (logical right shift by register)
pub const SHR_RR: MOpcode = MOpcode(0x32);
/// `shr dst, imm8`
pub const SHR_RI: MOpcode = MOpcode(0x33);
/// `sar dst, cl`  (arithmetic right shift by register)
pub const SAR_RR: MOpcode = MOpcode(0x34);
/// `sar dst, imm8`
pub const SAR_RI: MOpcode = MOpcode(0x35);

// ── comparisons ────────────────────────────────────────────────────────────
/// Public API for `CMP_RR`.
pub const CMP_RR: MOpcode = MOpcode(0x40);
/// Public API for `CMP_RI`.
pub const CMP_RI: MOpcode = MOpcode(0x41);
/// Public API for `TEST_RR`.
pub const TEST_RR: MOpcode = MOpcode(0x42);
/// `setcc dst`  — condition code stored as `Imm(CC_*)` in first operand.
pub const SETCC: MOpcode = MOpcode(0x43);

// ── control flow ───────────────────────────────────────────────────────────
/// Public API for `JMP`.
pub const JMP: MOpcode = MOpcode(0x50);
/// `jcc target`  — condition code stored as `Imm(CC_*)`, target as `Block`.
pub const JCC: MOpcode = MOpcode(0x51);
/// `call rel32`
pub const CALL_DIRECT: MOpcode = MOpcode(0x52);
/// `call *reg`
pub const CALL_R: MOpcode = MOpcode(0x53);
/// Public API for `RET`.
pub const RET: MOpcode = MOpcode(0x54);
/// Unconditional tail-call jump to a register-held callee address.
/// Emitted in place of `CALL_R` when the call is in tail position.
/// Encodes as a `jmp *reg` (FF /4) — no stack frame manipulation.
pub const JMP_TAIL: MOpcode = MOpcode(0x55);

// ── stack ──────────────────────────────────────────────────────────────────
/// Public API for `PUSH_R`.
pub const PUSH_R: MOpcode = MOpcode(0x60);
/// Public API for `POP_R`.
pub const POP_R: MOpcode = MOpcode(0x61);

// ── miscellaneous ──────────────────────────────────────────────────────────
/// Public API for `NOP`.
pub const NOP: MOpcode = MOpcode(0x70);
/// Raw inline assembly bytes emitted verbatim after minimal template decoding.
pub const INLINE_ASM: MOpcode = MOpcode(0x74);
/// `lea dst, [base + imm]`  — imm stored as `Imm` operand.
pub const LEA_RI: MOpcode = MOpcode(0x71);

// ── frame (spill) access ───────────────────────────────────────────────────
/// `mov dst, [rbp + disp]`  — spill reload.  `dst` = VReg, `operands[0]` = `Imm(slot)`.
pub const MOV_LOAD_MR: MOpcode = MOpcode(0x72);
/// `mov [rbp + disp], src`  — spill store.  `dst` = None, `operands[0]` = `Imm(slot)`, `operands[1]` = VReg/PReg(src).
pub const MOV_STORE_RM: MOpcode = MOpcode(0x73);

// ── SIMD / vector (SSE4.2 baseline for vector lowering) ───────────────────
/// Public API for `PADDD_RR`.
pub const PADDD_RR: MOpcode = MOpcode(0x80);
/// Public API for `PSUBD_RR`.
pub const PSUBD_RR: MOpcode = MOpcode(0x81);
/// Public API for `PMULLD_RR`.
pub const PMULLD_RR: MOpcode = MOpcode(0x82);
/// Public API for `ADDPS_RR`.
pub const ADDPS_RR: MOpcode = MOpcode(0x83);
/// Public API for `MULPS_RR`.
pub const MULPS_RR: MOpcode = MOpcode(0x84);
/// Public API for `DIVPS_RR`.
pub const DIVPS_RR: MOpcode = MOpcode(0x85);
/// Public API for `ADDPD_RR`.
pub const ADDPD_RR: MOpcode = MOpcode(0x86);
/// Public API for `MULPD_RR`.
pub const MULPD_RR: MOpcode = MOpcode(0x87);
/// Public API for `MOVAPS_RR`.
pub const MOVAPS_RR: MOpcode = MOpcode(0x88);
/// Public API for `MOVDQU_LOAD_MR`.
pub const MOVDQU_LOAD_MR: MOpcode = MOpcode(0x89);
/// Public API for `MOVDQU_STORE_RM`.
pub const MOVDQU_STORE_RM: MOpcode = MOpcode(0x8A);
/// Public API for `MOVAPS_LOAD_MR`.
pub const MOVAPS_LOAD_MR: MOpcode = MOpcode(0x8B);

// ── non-promotable memory: frame-slot + register-indirect ─────────────────
/// `lea dst, [rbp - (slot+1)*8]` — materialize a non-promotable alloca address.
/// `dst` = VReg, `operands[0]` = `Imm(slot_idx)`.
pub const LEA_FRAME_MR: MOpcode = MOpcode(0xA0);
/// `mov dst, [ptr_reg]` — 64-bit load through a register-held pointer.
/// `dst` = VReg, `operands[0]` = PReg/VReg (pointer register).
pub const MOV_LOAD_REG_MR: MOpcode = MOpcode(0xA1);
/// `mov [ptr_reg], src` — 64-bit store through a register-held pointer.
/// No `dst`. `operands[0]` = PReg/VReg (pointer), `operands[1]` = PReg/VReg (value).
pub const MOV_STORE_REG_RM: MOpcode = MOpcode(0xA2);

// ── atomic memory operations ───────────────────────────────────────────────
/// `mfence` — full memory barrier (0F AE F0).
pub const MFENCE: MOpcode = MOpcode(0x90);
/// `lock cmpxchg [ptr], new_val` — compare-and-swap (F0 REX.W 0F B1 /r).
/// operands[0]=ptr_preg, operands[1]=RAX (comparand, pre-loaded), operands[2]=new_val_preg.
pub const LOCK_CMPXCHG_MR: MOpcode = MOpcode(0x91);
/// `lock xadd [ptr], val` — atomic exchange-add, 64-bit (F0 REX.W 0F C1 /r).
/// operands[0]=ptr_preg, operands[1]=val_preg; old value returned in val_preg.
pub const LOCK_XADD_MR: MOpcode = MOpcode(0x92);
/// `lock xadd [ptr], val` — atomic exchange-add, 32-bit (F0 [REX] 0F C1 /r, no REX.W).
/// operands[0]=ptr_preg, operands[1]=val_preg; old value returned in val_preg.
pub const LOCK_XADD32_MR: MOpcode = MOpcode(0x9A);
/// `xchg [ptr], val` — atomic exchange, implicitly LOCK (REX.W 87 /r).
/// operands[0]=ptr_preg, operands[1]=val_preg; old value returned in val_preg.
pub const XCHG_MR: MOpcode = MOpcode(0x93);
/// `lock and [ptr], val` — atomic bitwise AND (F0 REX.W 21 /r).
/// operands[0]=ptr_preg, operands[1]=val_preg.
pub const LOCK_AND_MR: MOpcode = MOpcode(0x94);
/// `lock or [ptr], val` — atomic bitwise OR (F0 REX.W 09 /r).
/// operands[0]=ptr_preg, operands[1]=val_preg.
pub const LOCK_OR_MR: MOpcode = MOpcode(0x95);
/// `lock xor [ptr], val` — atomic bitwise XOR (F0 REX.W 31 /r).
/// operands[0]=ptr_preg, operands[1]=val_preg.
pub const LOCK_XOR_MR: MOpcode = MOpcode(0x96);

// ── SSE2 scalar double (prefix 66 0F) ─────────────────────────────────────
/// `addsd dst, src`  (66 0F 58 /r)
pub const ADDSD_RR: MOpcode = MOpcode(0xB0);
/// `subsd dst, src`  (66 0F 5C /r)
pub const SUBSD_RR: MOpcode = MOpcode(0xB1);
/// `mulsd dst, src`  (66 0F 59 /r)
pub const MULSD_RR: MOpcode = MOpcode(0xB2);
/// `divsd dst, src`  (66 0F 5E /r)
pub const DIVSD_RR: MOpcode = MOpcode(0xB3);
/// `sqrtsd dst, src` (66 0F 51 /r)
pub const SQRTSD_R: MOpcode = MOpcode(0xB4);
/// `ucomisd lhs, rhs` (66 0F 2E /r) — sets ZF/PF/CF; used before Jcc for FP branches.
pub const UCOMISD_RR: MOpcode = MOpcode(0xB5);
/// `movsd dst, [rbp+disp]` — FP spill reload.  `dst` = VReg, `operands[0]` = `Imm(slot)`.
pub const MOVSD_LOAD_MR: MOpcode = MOpcode(0xB6);
/// `movsd [rbp+disp], src` — FP spill store.  `dst` = None, `operands[0]` = `Imm(slot)`, `operands[1]` = VReg/PReg(src).
pub const MOVSD_STORE_RM: MOpcode = MOpcode(0xB7);

// ── SSE2 scalar single (prefix F3 0F) ─────────────────────────────────────
/// `addss dst, src`  (F3 0F 58 /r)
pub const ADDSS_RR: MOpcode = MOpcode(0xC0);
/// `subss dst, src`  (F3 0F 5C /r)
pub const SUBSS_RR: MOpcode = MOpcode(0xC1);
/// `mulss dst, src`  (F3 0F 59 /r)
pub const MULSS_RR: MOpcode = MOpcode(0xC2);
/// `divss dst, src`  (F3 0F 5E /r)
pub const DIVSS_RR: MOpcode = MOpcode(0xC3);
/// `sqrtss dst, src` (F3 0F 51 /r)
pub const SQRTSS_R: MOpcode = MOpcode(0xC4);
/// `ucomiss lhs, rhs` (0F 2E /r) — sets ZF/PF/CF; used before Jcc for FP branches.
pub const UCOMISS_RR: MOpcode = MOpcode(0xC5);
/// `movss dst, [rbp+disp]` — FP spill reload (f32).
pub const MOVSS_LOAD_MR: MOpcode = MOpcode(0xC6);
/// `movss [rbp+disp], src` — FP spill store (f32).
pub const MOVSS_STORE_RM: MOpcode = MOpcode(0xC7);

// ── FP ↔ integer conversions ───────────────────────────────────────────────
/// `cvttsd2si dst, src`  (F2 0F 2C /r) — f64 → i64, truncate toward zero.
pub const CVTTSD2SI_RR: MOpcode = MOpcode(0xD0);
/// `cvtsi2sd dst, src`   (F2 0F 2A /r) — i64 → f64.
pub const CVTSI2SD_RR: MOpcode = MOpcode(0xD1);
/// `cvttss2si dst, src`  (F3 0F 2C /r) — f32 → i32, truncate toward zero.
pub const CVTTSS2SI_RR: MOpcode = MOpcode(0xD2);
/// `cvtsi2ss dst, src`   (F3 0F 2A /r) — i32/i64 → f32.
pub const CVTSI2SS_RR: MOpcode = MOpcode(0xD3);
/// `cvtsd2ss dst, src`   (F2 0F 5A /r) — f64 → f32 (FPTrunc).
pub const CVTSD2SS_RR: MOpcode = MOpcode(0xD4);
/// `cvtss2sd dst, src`   (F3 0F 5A /r) — f32 → f64 (FPExt).
pub const CVTSS2SD_RR: MOpcode = MOpcode(0xD5);

// ── FP copy ────────────────────────────────────────────────────────────────
/// `movapd dst, src` (66 0F 28 /r) — XMM register copy (f64).
pub const MOVAPD_RR: MOpcode = MOpcode(0xD6);
/// `movaps dst, src` (0F 28 /r) — XMM register copy (f32).
pub const MOVAPD_RR_F32: MOpcode = MOpcode(0xD7);

// ── condition codes (used as Imm operands with JCC / SETCC) ────────────────
/// Public API for `CC_EQ`.
pub const CC_EQ: i64 = 0; // je  / jz
/// Public API for `CC_NE`.
pub const CC_NE: i64 = 1; // jne / jnz
/// Public API for `CC_LT`.
pub const CC_LT: i64 = 2; // jl  (signed)
/// Public API for `CC_LE`.
pub const CC_LE: i64 = 3; // jle (signed)
/// Public API for `CC_GT`.
pub const CC_GT: i64 = 4; // jg  (signed)
/// Public API for `CC_GE`.
pub const CC_GE: i64 = 5; // jge (signed)
/// Public API for `CC_ULT`.
pub const CC_ULT: i64 = 6; // jb  (unsigned below)
/// Public API for `CC_ULE`.
pub const CC_ULE: i64 = 7; // jbe (unsigned below-or-equal)
/// Public API for `CC_UGT`.
pub const CC_UGT: i64 = 8; // ja  (unsigned above)
/// Public API for `CC_UGE`.
pub const CC_UGE: i64 = 9; // jae (unsigned above-or-equal)
