//! x86_64 physical register definitions.

use llvm_codegen::isel::PReg;

// ── GPR definitions (REX encoding: 0-7 = rax-rdi, 8-15 = r8-r15) ──────────

/// Public API for `RAX`.
pub const RAX: PReg = PReg(0);
/// Public API for `RCX`.
pub const RCX: PReg = PReg(1);
/// Public API for `RDX`.
pub const RDX: PReg = PReg(2);
/// Public API for `RBX`.
pub const RBX: PReg = PReg(3);
/// Public API for `RSP`.
pub const RSP: PReg = PReg(4);
/// Public API for `RBP`.
pub const RBP: PReg = PReg(5);
/// Public API for `RSI`.
pub const RSI: PReg = PReg(6);
/// Public API for `RDI`.
pub const RDI: PReg = PReg(7);
/// Public API for `R8`.
pub const R8: PReg = PReg(8);
/// Public API for `R9`.
pub const R9: PReg = PReg(9);
/// Public API for `R10`.
pub const R10: PReg = PReg(10);
/// Public API for `R11`.
pub const R11: PReg = PReg(11);
/// Public API for `R12`.
pub const R12: PReg = PReg(12);
/// Public API for `R13`.
pub const R13: PReg = PReg(13);
/// Public API for `R14`.
pub const R14: PReg = PReg(14);
/// Public API for `R15`.
pub const R15: PReg = PReg(15);

/// Caller-saved registers available for allocation (excludes RSP, RBP and
/// the callee-saved set RBX, R12–R15).
pub const ALLOCATABLE: &[PReg] = &[RAX, RCX, RDX, RSI, RDI, R8, R9, R10, R11];

/// Callee-saved registers (must be preserved across calls per System V AMD64).
pub const CALLEE_SAVED: &[PReg] = &[RBX, RBP, R12, R13, R14, R15];

// ── XMM register definitions (SSE2 scalar FP; encodings 0-15) ────────────────

/// Public API for `XMM0`.
pub const XMM0: PReg = PReg(16);
/// Public API for `XMM1`.
pub const XMM1: PReg = PReg(17);
/// Public API for `XMM2`.
pub const XMM2: PReg = PReg(18);
/// Public API for `XMM3`.
pub const XMM3: PReg = PReg(19);
/// Public API for `XMM4`.
pub const XMM4: PReg = PReg(20);
/// Public API for `XMM5`.
pub const XMM5: PReg = PReg(21);
/// Public API for `XMM6`.
pub const XMM6: PReg = PReg(22);
/// Public API for `XMM7`.
pub const XMM7: PReg = PReg(23);
/// Public API for `XMM8`.
pub const XMM8: PReg = PReg(24);
/// Public API for `XMM9`.
pub const XMM9: PReg = PReg(25);
/// Public API for `XMM10`.
pub const XMM10: PReg = PReg(26);
/// Public API for `XMM11`.
pub const XMM11: PReg = PReg(27);
/// Public API for `XMM12`.
pub const XMM12: PReg = PReg(28);
/// Public API for `XMM13`.
pub const XMM13: PReg = PReg(29);
/// Public API for `XMM14`.
pub const XMM14: PReg = PReg(30);
/// Public API for `XMM15`.
pub const XMM15: PReg = PReg(31);

/// Caller-saved XMM registers available for FP allocation (System V AMD64).
/// XMM0–XMM7 are caller-saved; XMM8–XMM15 are callee-saved under Win64.
pub const FP_ALLOCATABLE: &[PReg] = &[XMM0, XMM1, XMM2, XMM3, XMM4, XMM5, XMM6, XMM7];

/// Callee-saved XMM registers under Windows x64 ABI.
pub const FP_CALLEE_SAVED_WIN64: &[PReg] = &[XMM8, XMM9, XMM10, XMM11, XMM12, XMM13, XMM14, XMM15];

/// Whether `r` is an XMM (FP/SIMD) register.
#[inline]
pub fn is_fp_reg(r: PReg) -> bool {
    r.0 >= 16
}

/// The 4-bit XMM register number (0-15) used in SSE2 ModRM/REX encoding.
/// Panics in debug mode if `r` is not an XMM register.
#[inline]
pub fn xmm_enc(r: PReg) -> u8 {
    debug_assert!(is_fp_reg(r), "xmm_enc called on non-XMM register");
    r.0 - 16
}

/// Return the XMM register name.
pub fn xmm_name(r: PReg) -> &'static str {
    match r.0 {
        16 => "xmm0",
        17 => "xmm1",
        18 => "xmm2",
        19 => "xmm3",
        20 => "xmm4",
        21 => "xmm5",
        22 => "xmm6",
        23 => "xmm7",
        24 => "xmm8",
        25 => "xmm9",
        26 => "xmm10",
        27 => "xmm11",
        28 => "xmm12",
        29 => "xmm13",
        30 => "xmm14",
        31 => "xmm15",
        _ => "??",
    }
}

/// Return the 64-bit register name in AT&T / Intel syntax.
pub fn reg_name(r: PReg) -> &'static str {
    match r.0 {
        0 => "rax",
        1 => "rcx",
        2 => "rdx",
        3 => "rbx",
        4 => "rsp",
        5 => "rbp",
        6 => "rsi",
        7 => "rdi",
        8 => "r8",
        9 => "r9",
        10 => "r10",
        11 => "r11",
        12 => "r12",
        13 => "r13",
        14 => "r14",
        15 => "r15",
        _ => "??",
    }
}

/// Whether a register requires the REX prefix (R8–R15).
pub fn is_extended(r: PReg) -> bool {
    r.0 >= 8
}

/// The 3-bit encoding used in the ModRM reg/rm fields (low 3 bits).
pub fn reg_enc(r: PReg) -> u8 {
    r.0 & 7
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allocatable_does_not_contain_rsp_rbp() {
        assert!(!ALLOCATABLE.contains(&RSP));
        assert!(!ALLOCATABLE.contains(&RBP));
    }

    #[test]
    fn allocatable_and_callee_saved_disjoint() {
        for r in ALLOCATABLE {
            assert!(!CALLEE_SAVED.contains(r), "{} in both sets", reg_name(*r));
        }
    }

    #[test]
    fn reg_enc_low3_bits() {
        assert_eq!(reg_enc(RAX), 0);
        assert_eq!(reg_enc(R8), 0); // R8 = 8 & 7 = 0
        assert_eq!(reg_enc(R9), 1);
        assert_eq!(reg_enc(RDI), 7);
    }

    #[test]
    fn extended_flag() {
        assert!(!is_extended(RAX));
        assert!(!is_extended(RDI));
        assert!(is_extended(R8));
        assert!(is_extended(R15));
    }

    #[test]
    fn fp_allocatable_is_xmm0_through_xmm7() {
        assert_eq!(FP_ALLOCATABLE.len(), 8);
        assert_eq!(FP_ALLOCATABLE[0], XMM0);
        assert_eq!(FP_ALLOCATABLE[7], XMM7);
    }

    #[test]
    fn is_fp_reg_correct() {
        assert!(!is_fp_reg(RAX));
        assert!(!is_fp_reg(R15));
        assert!(is_fp_reg(XMM0));
        assert!(is_fp_reg(XMM15));
    }

    #[test]
    fn xmm_enc_0_to_15() {
        assert_eq!(xmm_enc(XMM0), 0);
        assert_eq!(xmm_enc(XMM7), 7);
        assert_eq!(xmm_enc(XMM8), 8);
        assert_eq!(xmm_enc(XMM15), 15);
    }
}
