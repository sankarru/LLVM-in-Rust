//! AArch64 physical register definitions.

use llvm_codegen::isel::PReg;

// ── General-purpose 64-bit registers (X0–X30, XZR) ──────────────────────────
//
// The AArch64 encoding uses 5-bit register fields (0–31).
// X31 is context-dependent: XZR (zero register) in most instructions,
// SP (stack pointer) in load/store addressing.

/// Public API for `X0`.
pub const X0: PReg = PReg(0);
/// Public API for `X1`.
pub const X1: PReg = PReg(1);
/// Public API for `X2`.
pub const X2: PReg = PReg(2);
/// Public API for `X3`.
pub const X3: PReg = PReg(3);
/// Public API for `X4`.
pub const X4: PReg = PReg(4);
/// Public API for `X5`.
pub const X5: PReg = PReg(5);
/// Public API for `X6`.
pub const X6: PReg = PReg(6);
/// Public API for `X7`.
pub const X7: PReg = PReg(7);
/// Public API for `X8`.
pub const X8: PReg = PReg(8);
/// Public API for `X9`.
pub const X9: PReg = PReg(9);
/// Public API for `X10`.
pub const X10: PReg = PReg(10);
/// Public API for `X11`.
pub const X11: PReg = PReg(11);
/// Public API for `X12`.
pub const X12: PReg = PReg(12);
/// Public API for `X13`.
pub const X13: PReg = PReg(13);
/// Public API for `X14`.
pub const X14: PReg = PReg(14);
/// Public API for `X15`.
pub const X15: PReg = PReg(15);
/// Public API for `X16`.
pub const X16: PReg = PReg(16); // IP0 (intra-procedure-call scratch)
/// Public API for `X17`.
pub const X17: PReg = PReg(17); // IP1
/// Public API for `X18`.
pub const X18: PReg = PReg(18); // platform register (reserved on some ABIs)
/// Public API for `X19`.
pub const X19: PReg = PReg(19);
/// Public API for `X20`.
pub const X20: PReg = PReg(20);
/// Public API for `X21`.
pub const X21: PReg = PReg(21);
/// Public API for `X22`.
pub const X22: PReg = PReg(22);
/// Public API for `X23`.
pub const X23: PReg = PReg(23);
/// Public API for `X24`.
pub const X24: PReg = PReg(24);
/// Public API for `X25`.
pub const X25: PReg = PReg(25);
/// Public API for `X26`.
pub const X26: PReg = PReg(26);
/// Public API for `X27`.
pub const X27: PReg = PReg(27);
/// Public API for `X28`.
pub const X28: PReg = PReg(28);
/// Public API for `X29`.
pub const X29: PReg = PReg(29); // frame pointer
/// Public API for `X30`.
pub const X30: PReg = PReg(30); // link register
/// Public API for `XZR`.
pub const XZR: PReg = PReg(31); // zero register (read: 0, write: discard)

/// Registers available for the register allocator.
/// Caller-saved (X0–X18) minus X18 (platform reserved), plus a subset of
/// callee-saved. We expose X0–X15 as the primary allocation pool.
pub const ALLOCATABLE: &[PReg] = &[
    X0, X1, X2, X3, X4, X5, X6, X7, X8, X9, X10, X11, X12, X13, X14, X15, X19, X20, X21, X22, X23,
    X24, X25, X26, X27, X28,
];

/// Registers that must be preserved across calls (AAPCS64).
pub const CALLEE_SAVED: &[PReg] = &[X19, X20, X21, X22, X23, X24, X25, X26, X27, X28, X29, X30];

/// Integer argument registers (AAPCS64: X0–X7).
pub const ARG_REGS: &[PReg] = &[X0, X1, X2, X3, X4, X5, X6, X7];

/// Integer return register.
pub const RET_REG: PReg = X0;

/// 5-bit register encoding (low 5 bits; always 0–31 for AArch64).
pub fn reg_enc(r: PReg) -> u8 {
    r.0 & 0x1F
}

/// Whether a register is in the "upper" range (X16+).  Some instruction
/// variants or compact encodings have restrictions on these.
pub fn is_extended(r: PReg) -> bool {
    r.0 >= 16 && r.0 < 32
}

// ── Floating-point / SIMD D-registers (D0–D31) ───────────────────────────
//
// AArch64 has 32 floating-point/SIMD registers V0–V31 (128-bit each).
// When used as scalar double-precision, they are named D0–D31 (64-bit view).
// We encode them as PReg(32)–PReg(63) so they live in a separate numeric
// range from the integer registers (PReg(0)–PReg(31)).

/// Public API for `D0`.
pub const D0: PReg = PReg(32);
/// Public API for `D1`.
pub const D1: PReg = PReg(33);
/// Public API for `D2`.
pub const D2: PReg = PReg(34);
/// Public API for `D3`.
pub const D3: PReg = PReg(35);
/// Public API for `D4`.
pub const D4: PReg = PReg(36);
/// Public API for `D5`.
pub const D5: PReg = PReg(37);
/// Public API for `D6`.
pub const D6: PReg = PReg(38);
/// Public API for `D7`.
pub const D7: PReg = PReg(39);
/// Public API for `D8`.
pub const D8: PReg = PReg(40);
/// Public API for `D9`.
pub const D9: PReg = PReg(41);
/// Public API for `D10`.
pub const D10: PReg = PReg(42);
/// Public API for `D11`.
pub const D11: PReg = PReg(43);
/// Public API for `D12`.
pub const D12: PReg = PReg(44);
/// Public API for `D13`.
pub const D13: PReg = PReg(45);
/// Public API for `D14`.
pub const D14: PReg = PReg(46);
/// Public API for `D15`.
pub const D15: PReg = PReg(47);
/// Public API for `D16`.
pub const D16: PReg = PReg(48);
/// Public API for `D17`.
pub const D17: PReg = PReg(49);
/// Public API for `D18`.
pub const D18: PReg = PReg(50);
/// Public API for `D19`.
pub const D19: PReg = PReg(51);
/// Public API for `D20`.
pub const D20: PReg = PReg(52);
/// Public API for `D21`.
pub const D21: PReg = PReg(53);
/// Public API for `D22`.
pub const D22: PReg = PReg(54);
/// Public API for `D23`.
pub const D23: PReg = PReg(55);
/// Public API for `D24`.
pub const D24: PReg = PReg(56);
/// Public API for `D25`.
pub const D25: PReg = PReg(57);
/// Public API for `D26`.
pub const D26: PReg = PReg(58);
/// Public API for `D27`.
pub const D27: PReg = PReg(59);
/// Public API for `D28`.
pub const D28: PReg = PReg(60);
/// Public API for `D29`.
pub const D29: PReg = PReg(61);
/// Public API for `D30`.
pub const D30: PReg = PReg(62);
/// Public API for `D31`.
pub const D31: PReg = PReg(63);

/// FP/SIMD registers available for allocation (caller-saved: D0–D7, D16–D23).
///
/// AAPCS64: D8–D15 are callee-saved; D24–D31 are callee-saved.
/// D0–D7 and D16–D23 are caller-saved (allocate freely).
pub const FP_ALLOCATABLE: &[PReg] = &[
    D0, D1, D2, D3, D4, D5, D6, D7,
    D16, D17, D18, D19, D20, D21, D22, D23,
];

/// FP/SIMD callee-saved registers (D8–D15, AAPCS64).
pub const FP_CALLEE_SAVED: &[PReg] = &[D8, D9, D10, D11, D12, D13, D14, D15];

/// FP argument / return registers (D0–D7).
pub const FP_ARG_REGS: &[PReg] = &[D0, D1, D2, D3, D4, D5, D6, D7];

/// FP return register.
pub const FP_RET_REG: PReg = D0;

/// Returns `true` if `r` is a floating-point / SIMD D-register (PReg ≥ 32).
#[inline]
pub fn is_fp_reg(r: PReg) -> bool {
    r.0 >= 32
}

/// 5-bit hardware register number for a D-register: `r.0 - 32`.
///
/// Panics in debug builds if `r` is not a D-register.
#[inline]
pub fn fp_enc(r: PReg) -> u8 {
    debug_assert!(is_fp_reg(r), "fp_enc called on non-FP register {:?}", r);
    r.0 - 32
}

/// Human-readable register name.
pub fn reg_name(r: PReg) -> &'static str {
    match r.0 {
        0 => "x0",
        1 => "x1",
        2 => "x2",
        3 => "x3",
        4 => "x4",
        5 => "x5",
        6 => "x6",
        7 => "x7",
        8 => "x8",
        9 => "x9",
        10 => "x10",
        11 => "x11",
        12 => "x12",
        13 => "x13",
        14 => "x14",
        15 => "x15",
        16 => "x16",
        17 => "x17",
        18 => "x18",
        19 => "x19",
        20 => "x20",
        21 => "x21",
        22 => "x22",
        23 => "x23",
        24 => "x24",
        25 => "x25",
        26 => "x26",
        27 => "x27",
        28 => "x28",
        29 => "x29",
        30 => "x30",
        31 => "xzr",
        _ => "??",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allocatable_does_not_contain_x29_x30_xzr() {
        assert!(!ALLOCATABLE.contains(&X29));
        assert!(!ALLOCATABLE.contains(&X30));
        assert!(!ALLOCATABLE.contains(&XZR));
    }

    #[test]
    fn arg_regs_are_x0_through_x7() {
        assert_eq!(ARG_REGS, &[X0, X1, X2, X3, X4, X5, X6, X7]);
    }

    #[test]
    fn ret_reg_is_x0() {
        assert_eq!(RET_REG, X0);
    }

    #[test]
    fn reg_enc_low5_bits() {
        assert_eq!(reg_enc(X0), 0);
        assert_eq!(reg_enc(X7), 7);
        assert_eq!(reg_enc(XZR), 31);
        assert_eq!(reg_enc(XZR), 31);
    }

    #[test]
    fn allocatable_and_callee_saved_overlap_correctly() {
        // X19-X28 must appear in BOTH ALLOCATABLE and CALLEE_SAVED.
        for r in &[X19, X20, X21, X22, X23, X24, X25, X26, X27, X28] {
            assert!(
                ALLOCATABLE.contains(r),
                "{} missing from ALLOCATABLE",
                reg_name(*r)
            );
            assert!(
                CALLEE_SAVED.contains(r),
                "{} missing from CALLEE_SAVED",
                reg_name(*r)
            );
        }
    }

    // ── FP / D-register tests ─────────────────────────────────────────────

    #[test]
    fn d0_is_preg_32() {
        assert_eq!(D0, PReg(32));
    }

    #[test]
    fn d31_is_preg_63() {
        assert_eq!(D31, PReg(63));
    }

    #[test]
    fn is_fp_reg_distinguishes_int_and_fp() {
        assert!(!is_fp_reg(X0));
        assert!(!is_fp_reg(XZR));
        assert!(is_fp_reg(D0));
        assert!(is_fp_reg(D15));
        assert!(is_fp_reg(D31));
    }

    #[test]
    fn fp_enc_returns_correct_hw_number() {
        assert_eq!(fp_enc(D0), 0);
        assert_eq!(fp_enc(D7), 7);
        assert_eq!(fp_enc(D16), 16);
        assert_eq!(fp_enc(D31), 31);
    }

    #[test]
    fn fp_allocatable_contains_caller_saved_only() {
        // D8–D15 are callee-saved and must NOT be in FP_ALLOCATABLE.
        for r in FP_CALLEE_SAVED {
            assert!(
                !FP_ALLOCATABLE.contains(r),
                "{r:?} is callee-saved but appears in FP_ALLOCATABLE"
            );
        }
        // D0–D7 and D16–D23 must be in FP_ALLOCATABLE.
        for r in &[D0, D1, D7, D16, D23] {
            assert!(
                FP_ALLOCATABLE.contains(r),
                "{r:?} should be in FP_ALLOCATABLE"
            );
        }
    }

    #[test]
    fn fp_arg_regs_are_d0_through_d7() {
        assert_eq!(FP_ARG_REGS, &[D0, D1, D2, D3, D4, D5, D6, D7]);
    }

    #[test]
    fn fp_ret_reg_is_d0() {
        assert_eq!(FP_RET_REG, D0);
    }
}
