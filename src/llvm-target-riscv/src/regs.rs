//! RISC-V RV64 integer and floating-point register definitions.

use llvm_codegen::isel::PReg;

/// Public API for `X0`.
pub const X0: PReg = PReg(0); // zero
/// Public API for `X1`.
pub const X1: PReg = PReg(1); // ra
/// Public API for `X2`.
pub const X2: PReg = PReg(2); // sp
/// Public API for `X3`.
pub const X3: PReg = PReg(3); // gp
/// Public API for `X4`.
pub const X4: PReg = PReg(4); // tp
/// Public API for `X5`.
pub const X5: PReg = PReg(5); // t0
/// Public API for `X6`.
pub const X6: PReg = PReg(6); // t1
/// Public API for `X7`.
pub const X7: PReg = PReg(7); // t2
/// Public API for `X8`.
pub const X8: PReg = PReg(8); // s0/fp
/// Public API for `X9`.
pub const X9: PReg = PReg(9); // s1
/// Public API for `X10`.
pub const X10: PReg = PReg(10); // a0
/// Public API for `X11`.
pub const X11: PReg = PReg(11); // a1
/// Public API for `X12`.
pub const X12: PReg = PReg(12); // a2
/// Public API for `X13`.
pub const X13: PReg = PReg(13); // a3
/// Public API for `X14`.
pub const X14: PReg = PReg(14); // a4
/// Public API for `X15`.
pub const X15: PReg = PReg(15); // a5
/// Public API for `X16`.
pub const X16: PReg = PReg(16); // a6
/// Public API for `X17`.
pub const X17: PReg = PReg(17); // a7
/// Public API for `X18`.
pub const X18: PReg = PReg(18); // s2
/// Public API for `X19`.
pub const X19: PReg = PReg(19); // s3
/// Public API for `X20`.
pub const X20: PReg = PReg(20); // s4
/// Public API for `X21`.
pub const X21: PReg = PReg(21); // s5
/// Public API for `X22`.
pub const X22: PReg = PReg(22); // s6
/// Public API for `X23`.
pub const X23: PReg = PReg(23); // s7
/// Public API for `X24`.
pub const X24: PReg = PReg(24); // s8
/// Public API for `X25`.
pub const X25: PReg = PReg(25); // s9
/// Public API for `X26`.
pub const X26: PReg = PReg(26); // s10
/// Public API for `X27`.
pub const X27: PReg = PReg(27); // s11
/// Public API for `X28`.
pub const X28: PReg = PReg(28); // t3
/// Public API for `X29`.
pub const X29: PReg = PReg(29); // t4
/// Public API for `X30`.
pub const X30: PReg = PReg(30); // t5
/// Public API for `X31`.
pub const X31: PReg = PReg(31); // t6

/// Public API for `ARG_REGS`.
pub const ARG_REGS: &[PReg] = &[X10, X11, X12, X13, X14, X15, X16, X17];
/// Public API for `RET_REG`.
pub const RET_REG: PReg = X10;

/// Public API for `CALLEE_SAVED`.
pub const CALLEE_SAVED: &[PReg] = &[X9, X18, X19, X20, X21, X22, X23, X24, X25, X26, X27];

/// Public API for `ALLOCATABLE`.
pub const ALLOCATABLE: &[PReg] = &[
    X10, X11, X12, X13, X14, X15, X16, X17, // a0-a7
    X5, X6, X7, X28, X29, X30, X31, // t0-t6
    X9, X18, X19, X20, X21, X22, X23, X24, X25, X26, X27, // s1,s2-s11
];

/// Public API for `reg_enc`.
pub fn reg_enc(r: PReg) -> u8 {
    r.0 & 0x1F
}

// ── F/D extension floating-point registers (F0-F31 = PReg(32)-PReg(63)) ────

/// Public API for `F0` (ft0).
pub const F0:  PReg = PReg(32);
/// Public API for `F1` (ft1).
pub const F1:  PReg = PReg(33);
/// Public API for `F2` (ft2).
pub const F2:  PReg = PReg(34);
/// Public API for `F3` (ft3).
pub const F3:  PReg = PReg(35);
/// Public API for `F4` (ft4).
pub const F4:  PReg = PReg(36);
/// Public API for `F5` (ft5).
pub const F5:  PReg = PReg(37);
/// Public API for `F6` (ft6).
pub const F6:  PReg = PReg(38);
/// Public API for `F7` (ft7).
pub const F7:  PReg = PReg(39);
/// Public API for `F8` (fs0).
pub const F8:  PReg = PReg(40);
/// Public API for `F9` (fs1).
pub const F9:  PReg = PReg(41);
/// Public API for `F10` (fa0).
pub const F10: PReg = PReg(42);
/// Public API for `F11` (fa1).
pub const F11: PReg = PReg(43);
/// Public API for `F12` (fa2).
pub const F12: PReg = PReg(44);
/// Public API for `F13` (fa3).
pub const F13: PReg = PReg(45);
/// Public API for `F14` (fa4).
pub const F14: PReg = PReg(46);
/// Public API for `F15` (fa5).
pub const F15: PReg = PReg(47);
/// Public API for `F16` (fa6).
pub const F16: PReg = PReg(48);
/// Public API for `F17` (fa7).
pub const F17: PReg = PReg(49);
/// Public API for `F18` (fs2).
pub const F18: PReg = PReg(50);
/// Public API for `F19` (fs3).
pub const F19: PReg = PReg(51);
/// Public API for `F20` (fs4).
pub const F20: PReg = PReg(52);
/// Public API for `F21` (fs5).
pub const F21: PReg = PReg(53);
/// Public API for `F22` (fs6).
pub const F22: PReg = PReg(54);
/// Public API for `F23` (fs7).
pub const F23: PReg = PReg(55);
/// Public API for `F24` (fs8).
pub const F24: PReg = PReg(56);
/// Public API for `F25` (fs9).
pub const F25: PReg = PReg(57);
/// Public API for `F26` (fs10).
pub const F26: PReg = PReg(58);
/// Public API for `F27` (fs11).
pub const F27: PReg = PReg(59);
/// Public API for `F28` (ft8).
pub const F28: PReg = PReg(60);
/// Public API for `F29` (ft9).
pub const F29: PReg = PReg(61);
/// Public API for `F30` (ft10).
pub const F30: PReg = PReg(62);
/// Public API for `F31` (ft11).
pub const F31: PReg = PReg(63);

/// FP argument registers fa0-fa7 (F10-F17).
pub const FP_ARG_REGS: &[PReg] = &[F10, F11, F12, F13, F14, F15, F16, F17];

/// FP return register fa0 (F10).
pub const FP_RET_REG: PReg = F10;

/// FP callee-saved registers fs0-fs11 (F8-F9, F18-F27).
pub const FP_CALLEE_SAVED: &[PReg] = &[F8, F9, F18, F19, F20, F21, F22, F23, F24, F25, F26, F27];

/// All FP registers available for allocation:
/// ft0-ft7 (F0-F7, caller-saved temporaries),
/// fa0-fa7 (F10-F17, caller-saved args/return),
/// ft8-ft11 (F28-F31, caller-saved temporaries).
/// fs0-fs11 (F8-F9, F18-F27) are callee-saved and included for completeness.
pub const FP_ALLOCATABLE: &[PReg] = &[
    F0, F1, F2, F3, F4, F5, F6, F7,     // ft0-ft7 (caller-saved temporaries)
    F10, F11, F12, F13, F14, F15, F16, F17, // fa0-fa7 (caller-saved args)
    F28, F29, F30, F31,                   // ft8-ft11 (caller-saved temporaries)
    F8, F9, F18, F19, F20, F21, F22, F23, F24, F25, F26, F27, // fs0-fs11 (callee-saved)
];

/// Returns `true` if `r` is an FP register (F0-F31).
#[inline]
pub fn is_fp_reg(r: PReg) -> bool {
    r.0 >= 32
}

/// Returns the 5-bit hardware register number for an FP register.
///
/// FP registers are allocated with `PReg(32..=63)`; the hardware encoding
/// is `r.0 - 32` (0..=31).
#[inline]
pub fn fp_enc(r: PReg) -> u8 {
    r.0 - 32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arg_regs_are_a0_to_a7() {
        assert_eq!(ARG_REGS, &[X10, X11, X12, X13, X14, X15, X16, X17]);
    }

    #[test]
    fn ret_reg_is_a0() {
        assert_eq!(RET_REG, X10);
    }

    #[test]
    fn allocatable_excludes_zero_sp_gp_tp_ra_fp() {
        for r in [X0, X1, X2, X3, X4, X8] {
            assert!(!ALLOCATABLE.contains(&r));
        }
    }

    #[test]
    fn callee_saved_subset_in_allocatable() {
        for r in CALLEE_SAVED {
            assert!(ALLOCATABLE.contains(r));
        }
    }

    #[test]
    fn fp_reg_numbering() {
        assert_eq!(F0.0, 32);
        assert_eq!(F31.0, 63);
        assert_eq!(F10.0, 42); // fa0
    }

    #[test]
    fn fp_enc_returns_hardware_number() {
        assert_eq!(fp_enc(F0), 0);
        assert_eq!(fp_enc(F10), 10);
        assert_eq!(fp_enc(F31), 31);
    }

    #[test]
    fn is_fp_reg_distinguishes_int_fp() {
        assert!(!is_fp_reg(X0));
        assert!(!is_fp_reg(X31));
        assert!(is_fp_reg(F0));
        assert!(is_fp_reg(F31));
    }

    #[test]
    fn fp_allocatable_contains_fa0() {
        assert!(FP_ALLOCATABLE.contains(&F10));
    }

    #[test]
    fn fp_ret_reg_is_fa0() {
        assert_eq!(FP_RET_REG, F10);
    }

    #[test]
    fn fp_arg_regs_are_fa0_to_fa7() {
        assert_eq!(FP_ARG_REGS, &[F10, F11, F12, F13, F14, F15, F16, F17]);
    }
}
