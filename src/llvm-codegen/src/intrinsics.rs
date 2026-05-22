//! LLVM intrinsic recognition and lowering.
//!
//! Intrinsics are special LLVM function calls whose names begin with `llvm.`.
//! This module provides:
//! - [`is_llvm_intrinsic`] — recognise intrinsic call names
//! - [`lower_intrinsic`] — lower a known intrinsic to machine instructions

use crate::isel::{MInstr, MOpcode, MOperand, MachineFunction};

// ── public API ──────────────────────────────────────────────────────────────

/// Return `true` if `name` is an LLVM intrinsic (starts with `"llvm."`).
///
/// # Examples
/// ```
/// use llvm_codegen::is_llvm_intrinsic;
/// assert!(is_llvm_intrinsic("llvm.memcpy.p0.p0.i64"));
/// assert!(is_llvm_intrinsic("llvm.trap"));
/// assert!(!is_llvm_intrinsic("memcpy"));
/// ```
pub fn is_llvm_intrinsic(name: &str) -> bool {
    name.starts_with("llvm.")
}

/// Lower a known LLVM intrinsic to machine instructions, appending them to
/// `mf.blocks[block]`.
///
/// Returns `true` if the intrinsic was handled (including no-ops), `false` if
/// the intrinsic is not recognised and the caller should fall back to a generic
/// call.
///
/// # Handled intrinsics
///
/// | Pattern | Lowering |
/// |---------|----------|
/// | `llvm.memcpy.*` | libcall to `memcpy` |
/// | `llvm.memmove.*` | libcall to `memmove` |
/// | `llvm.memset.*` | libcall to `memset` |
/// | `llvm.assume` | NOP (dropped) |
/// | `llvm.lifetime.start.*` | NOP (dropped) |
/// | `llvm.lifetime.end.*` | NOP (dropped) |
/// | `llvm.expect.*` | NOP (branch-hint; value flows through) |
/// | `llvm.trap` | UD2 (undefined instruction) |
/// | `llvm.debugtrap` | UD2 |
pub fn lower_intrinsic(
    name: &str,
    args: &[MOperand],
    mf: &mut MachineFunction,
    block: usize,
) -> bool {
    if name.starts_with("llvm.memcpy") {
        emit_libcall(mf, block, "memcpy", args);
        return true;
    }
    if name.starts_with("llvm.memmove") {
        emit_libcall(mf, block, "memmove", args);
        return true;
    }
    if name.starts_with("llvm.memset") {
        emit_libcall(mf, block, "memset", args);
        return true;
    }
    if name.starts_with("llvm.assume")
        || name.starts_with("llvm.lifetime.start")
        || name.starts_with("llvm.lifetime.end")
        || name.starts_with("llvm.expect")
    {
        // These are hints/markers; drop them.
        return true;
    }
    if name == "llvm.trap" || name == "llvm.debugtrap" {
        emit_ud2(mf, block);
        return true;
    }
    false
}

// ── private helpers ──────────────────────────────────────────────────────────

/// Opcode used to represent a raw-bytes machine instruction (target emits verbatim).
const OPCODE_RAW_BYTES: MOpcode = MOpcode(0xFFFF_FFFE);

/// Opcode used to represent an indirect/named libcall.
const OPCODE_LIBCALL: MOpcode = MOpcode(0xFFFF_FFFD);

/// Emit a NOP-call to a named libc function with the given operands.
///
/// The emitted instruction carries the callee name as a `Bytes` operand
/// (null-terminated) followed by the call arguments. Target encoders recognise
/// `OPCODE_LIBCALL` and emit a PC-relative call with a relocation.
fn emit_libcall(mf: &mut MachineFunction, block: usize, callee: &str, args: &[MOperand]) {
    let mut instr = MInstr::new(OPCODE_LIBCALL);
    // Encode the callee name as a null-terminated byte string in the first operand.
    let mut name_bytes = callee.as_bytes().to_vec();
    name_bytes.push(0);
    instr.operands.push(MOperand::Bytes(name_bytes));
    for arg in args {
        instr.operands.push(arg.clone());
    }
    mf.push(block, instr);
}

/// Emit a UD2 instruction (x86-64 undefined instruction that raises SIGILL).
///
/// Encoded as raw bytes `[0x0F, 0x0B]` inside a `OPCODE_RAW_BYTES` MInstr.
fn emit_ud2(mf: &mut MachineFunction, block: usize) {
    let mut instr = MInstr::new(OPCODE_RAW_BYTES);
    instr.operands.push(MOperand::Bytes(vec![0x0F, 0x0B]));
    mf.push(block, instr);
}

// ── tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::isel::MachineFunction;

    fn empty_mf() -> MachineFunction {
        let mut mf = MachineFunction::new("test".into());
        mf.add_block("entry");
        mf
    }

    // ── is_llvm_intrinsic ────────────────────────────────────────────────────

    #[test]
    fn recognises_memcpy() {
        assert!(is_llvm_intrinsic("llvm.memcpy.p0.p0.i64"));
    }

    #[test]
    fn recognises_memset() {
        assert!(is_llvm_intrinsic("llvm.memset.p0.i64"));
    }

    #[test]
    fn recognises_memmove() {
        assert!(is_llvm_intrinsic("llvm.memmove.p0.p0.i64"));
    }

    #[test]
    fn recognises_assume() {
        assert!(is_llvm_intrinsic("llvm.assume"));
    }

    #[test]
    fn recognises_trap() {
        assert!(is_llvm_intrinsic("llvm.trap"));
    }

    #[test]
    fn recognises_lifetime() {
        assert!(is_llvm_intrinsic("llvm.lifetime.start.p0"));
        assert!(is_llvm_intrinsic("llvm.lifetime.end.p0"));
    }

    #[test]
    fn rejects_plain_memcpy() {
        assert!(!is_llvm_intrinsic("memcpy"));
    }

    #[test]
    fn rejects_printf() {
        assert!(!is_llvm_intrinsic("printf"));
    }

    #[test]
    fn rejects_empty_string() {
        assert!(!is_llvm_intrinsic(""));
    }

    // ── lower_intrinsic ──────────────────────────────────────────────────────

    #[test]
    fn assume_is_nop() {
        let mut mf = empty_mf();
        let handled = lower_intrinsic("llvm.assume", &[], &mut mf, 0);
        assert!(handled);
        assert!(mf.blocks[0].instrs.is_empty(), "assume should emit no instructions");
    }

    #[test]
    fn lifetime_start_is_nop() {
        let mut mf = empty_mf();
        let handled = lower_intrinsic("llvm.lifetime.start.p0", &[], &mut mf, 0);
        assert!(handled);
        assert!(mf.blocks[0].instrs.is_empty());
    }

    #[test]
    fn lifetime_end_is_nop() {
        let mut mf = empty_mf();
        let handled = lower_intrinsic("llvm.lifetime.end.p0", &[], &mut mf, 0);
        assert!(handled);
        assert!(mf.blocks[0].instrs.is_empty());
    }

    #[test]
    fn expect_is_nop() {
        let mut mf = empty_mf();
        let handled = lower_intrinsic("llvm.expect.i1", &[], &mut mf, 0);
        assert!(handled);
        assert!(mf.blocks[0].instrs.is_empty());
    }

    #[test]
    fn trap_emits_ud2() {
        let mut mf = empty_mf();
        let handled = lower_intrinsic("llvm.trap", &[], &mut mf, 0);
        assert!(handled);
        assert_eq!(mf.blocks[0].instrs.len(), 1);
        let instr = &mf.blocks[0].instrs[0];
        assert_eq!(instr.opcode, OPCODE_RAW_BYTES);
        match &instr.operands[0] {
            MOperand::Bytes(b) => assert_eq!(b.as_slice(), &[0x0F, 0x0B]),
            _ => panic!("expected Bytes operand"),
        }
    }

    #[test]
    fn debugtrap_emits_ud2() {
        let mut mf = empty_mf();
        let handled = lower_intrinsic("llvm.debugtrap", &[], &mut mf, 0);
        assert!(handled);
        assert_eq!(mf.blocks[0].instrs.len(), 1);
    }

    #[test]
    fn memcpy_emits_libcall() {
        let mut mf = empty_mf();
        let args = vec![MOperand::Imm(0), MOperand::Imm(1), MOperand::Imm(8)];
        let handled = lower_intrinsic("llvm.memcpy.p0.p0.i64", &args, &mut mf, 0);
        assert!(handled);
        assert_eq!(mf.blocks[0].instrs.len(), 1);
        let instr = &mf.blocks[0].instrs[0];
        assert_eq!(instr.opcode, OPCODE_LIBCALL);
        match &instr.operands[0] {
            MOperand::Bytes(b) => {
                assert!(b.starts_with(b"memcpy"), "callee name must be memcpy");
                assert_eq!(*b.last().unwrap(), 0, "must be null-terminated");
            }
            _ => panic!("first operand must be callee name bytes"),
        }
        assert_eq!(instr.operands.len(), 4, "name + 3 args");
    }

    #[test]
    fn memmove_emits_libcall() {
        let mut mf = empty_mf();
        let handled = lower_intrinsic("llvm.memmove.p0.p0.i64", &[], &mut mf, 0);
        assert!(handled);
        match &mf.blocks[0].instrs[0].operands[0] {
            MOperand::Bytes(b) => assert!(b.starts_with(b"memmove")),
            _ => panic!("expected callee name"),
        }
    }

    #[test]
    fn memset_emits_libcall() {
        let mut mf = empty_mf();
        let handled = lower_intrinsic("llvm.memset.p0.i64", &[], &mut mf, 0);
        assert!(handled);
        match &mf.blocks[0].instrs[0].operands[0] {
            MOperand::Bytes(b) => assert!(b.starts_with(b"memset")),
            _ => panic!("expected callee name"),
        }
    }

    #[test]
    fn unknown_intrinsic_returns_false() {
        let mut mf = empty_mf();
        let handled = lower_intrinsic("llvm.unknown_xyz_42", &[], &mut mf, 0);
        assert!(!handled, "unknown intrinsics must return false");
        assert!(mf.blocks[0].instrs.is_empty(), "no instructions emitted for unknown");
    }

    #[test]
    fn non_intrinsic_returns_false() {
        let mut mf = empty_mf();
        let handled = lower_intrinsic("printf", &[], &mut mf, 0);
        assert!(!handled);
    }
}
