//! RISC-V calling convention helpers (rv64 psABI subset).
//!
//! Covers both the integer ABI (a0-a7 / x10-x17) and the hardware floating-point
//! ABI (fa0-fa7 / f10-f17).  The hardware FP ABI passes `float` and `double`
//! arguments in dedicated FP registers when registers are available.

use crate::regs::{ARG_REGS, FP_ARG_REGS, FP_RET_REG, RET_REG};
use llvm_codegen::isel::PReg;

/// Public API for `ArgLocation`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ArgLocation {
    /// `Reg` variant.
    Reg(PReg),
    /// `Stack` variant.
    Stack(i32),
}

/// Classify integer arguments per the RV64 integer ABI (a0-a7, then stack).
pub fn classify_rv64_int_args(n_args: usize) -> Vec<ArgLocation> {
    (0..n_args)
        .map(|i| {
            if i < ARG_REGS.len() {
                ArgLocation::Reg(ARG_REGS[i])
            } else {
                ArgLocation::Stack(((i - ARG_REGS.len()) * 8) as i32)
            }
        })
        .collect()
}

/// Classify floating-point arguments per the RV64 hardware FP ABI (fa0-fa7, then stack).
pub fn classify_rv64_fp_args(n_args: usize) -> Vec<ArgLocation> {
    (0..n_args)
        .map(|i| {
            if i < FP_ARG_REGS.len() {
                ArgLocation::Reg(FP_ARG_REGS[i])
            } else {
                ArgLocation::Stack(((i - FP_ARG_REGS.len()) * 8) as i32)
            }
        })
        .collect()
}

/// Integer return register (a0 / x10).
pub const INT_RET: PReg = RET_REG;

/// FP return register (fa0 / f10).
pub const FP_RET: PReg = FP_RET_REG;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::regs::{F10, F11, F12, F13, F14, F15, F16, F17, X10, X11, X12, X13, X14, X15, X16, X17};

    #[test]
    fn first_eight_go_to_a0_a7() {
        let locs = classify_rv64_int_args(8);
        assert_eq!(locs[0], ArgLocation::Reg(X10));
        assert_eq!(locs[1], ArgLocation::Reg(X11));
        assert_eq!(locs[2], ArgLocation::Reg(X12));
        assert_eq!(locs[3], ArgLocation::Reg(X13));
        assert_eq!(locs[4], ArgLocation::Reg(X14));
        assert_eq!(locs[5], ArgLocation::Reg(X15));
        assert_eq!(locs[6], ArgLocation::Reg(X16));
        assert_eq!(locs[7], ArgLocation::Reg(X17));
    }

    #[test]
    fn ninth_arg_on_stack_0() {
        let locs = classify_rv64_int_args(9);
        assert_eq!(locs[8], ArgLocation::Stack(0));
    }

    #[test]
    fn tenth_arg_on_stack_8() {
        let locs = classify_rv64_int_args(10);
        assert_eq!(locs[9], ArgLocation::Stack(8));
    }

    #[test]
    fn zero_args_empty() {
        assert!(classify_rv64_int_args(0).is_empty());
    }

    #[test]
    fn fp_first_eight_go_to_fa0_fa7() {
        let locs = classify_rv64_fp_args(8);
        assert_eq!(locs[0], ArgLocation::Reg(F10));
        assert_eq!(locs[1], ArgLocation::Reg(F11));
        assert_eq!(locs[2], ArgLocation::Reg(F12));
        assert_eq!(locs[3], ArgLocation::Reg(F13));
        assert_eq!(locs[4], ArgLocation::Reg(F14));
        assert_eq!(locs[5], ArgLocation::Reg(F15));
        assert_eq!(locs[6], ArgLocation::Reg(F16));
        assert_eq!(locs[7], ArgLocation::Reg(F17));
    }

    #[test]
    fn fp_ninth_arg_on_stack() {
        let locs = classify_rv64_fp_args(9);
        assert_eq!(locs[8], ArgLocation::Stack(0));
    }

    #[test]
    fn fp_ret_is_fa0() {
        assert_eq!(FP_RET, F10);
    }
}
