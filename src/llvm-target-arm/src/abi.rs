//! AArch64 calling convention support (AAPCS64).

use crate::regs::{ARG_REGS, FP_ARG_REGS, RET_REG};
use llvm_codegen::isel::PReg;

// Re-export for convenience.
/// Public API for `re-export`.
pub use crate::regs::RET_REG as AAPCS64_INT_RET;

/// Whether a type is a floating-point type (float or double).
///
/// Used by the ABI classifier and lowering code to route values to the
/// correct register bank.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArgKind {
    /// Integer, pointer, or other non-FP type.
    Int,
    /// Floating-point scalar (float or double).
    Float,
}

/// Where a single argument lands after ABI classification.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ArgLocation {
    /// Passed in the given integer register.
    Reg(PReg),
    /// Passed on the stack at `offset` bytes above SP at the call site
    /// (first stack argument is at offset 0, aligned to 8 bytes).
    Stack(i32),
}

/// Classify `n_args` integer/pointer arguments using AAPCS64.
///
/// Arguments 0–7 go into X0–X7; arguments 8+ go on the stack.
pub fn classify_aapcs64_args(n_args: usize) -> Vec<ArgLocation> {
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

/// Classify arguments using AAPCS64, routing float/double args to D-registers.
///
/// Integer and pointer arguments consume from X0–X7 in order.
/// Floating-point arguments (float/double) consume from D0–D7 in order.
/// Integer and FP argument slots are *independent* per the AAPCS64 spec.
///
/// `kinds[i]` must be `ArgKind::Float` if and only if argument `i` is a
/// scalar floating-point type.
pub fn classify_aapcs64_args_typed(kinds: &[ArgKind]) -> Vec<ArgLocation> {
    let mut int_idx = 0usize;
    let mut fp_idx = 0usize;
    let mut stack_int_off = 0i32;
    let mut stack_fp_off = 0i32;

    kinds
        .iter()
        .map(|&kind| match kind {
            ArgKind::Float => {
                if fp_idx < FP_ARG_REGS.len() {
                    let loc = ArgLocation::Reg(FP_ARG_REGS[fp_idx]);
                    fp_idx += 1;
                    loc
                } else {
                    let off = stack_fp_off;
                    stack_fp_off += 8;
                    ArgLocation::Stack(off)
                }
            }
            ArgKind::Int => {
                if int_idx < ARG_REGS.len() {
                    let loc = ArgLocation::Reg(ARG_REGS[int_idx]);
                    int_idx += 1;
                    loc
                } else {
                    let off = stack_int_off;
                    stack_int_off += 8;
                    ArgLocation::Stack(off)
                }
            }
        })
        .collect()
}

/// The AAPCS64 integer return register (X0).
pub const INT_RET: PReg = RET_REG;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::regs::{X0, X1, X2, X3, X4, X5, X6, X7};
    use crate::regs::{D0, D1, D2, D7};

    #[test]
    fn first_eight_in_registers() {
        let locs = classify_aapcs64_args(8);
        assert_eq!(locs[0], ArgLocation::Reg(X0));
        assert_eq!(locs[1], ArgLocation::Reg(X1));
        assert_eq!(locs[2], ArgLocation::Reg(X2));
        assert_eq!(locs[3], ArgLocation::Reg(X3));
        assert_eq!(locs[4], ArgLocation::Reg(X4));
        assert_eq!(locs[5], ArgLocation::Reg(X5));
        assert_eq!(locs[6], ArgLocation::Reg(X6));
        assert_eq!(locs[7], ArgLocation::Reg(X7));
    }

    #[test]
    fn ninth_on_stack_at_offset_0() {
        let locs = classify_aapcs64_args(9);
        assert_eq!(locs[8], ArgLocation::Stack(0));
    }

    #[test]
    fn tenth_on_stack_at_offset_8() {
        let locs = classify_aapcs64_args(10);
        assert_eq!(locs[9], ArgLocation::Stack(8));
    }

    #[test]
    fn zero_args_is_empty() {
        assert!(classify_aapcs64_args(0).is_empty());
    }

    // ── typed ABI tests ───────────────────────────────────────────────────

    #[test]
    fn typed_float_args_go_to_d_regs() {
        let kinds = [ArgKind::Float, ArgKind::Float, ArgKind::Float];
        let locs = classify_aapcs64_args_typed(&kinds);
        assert_eq!(locs[0], ArgLocation::Reg(D0));
        assert_eq!(locs[1], ArgLocation::Reg(D1));
        assert_eq!(locs[2], ArgLocation::Reg(D2));
    }

    #[test]
    fn typed_int_args_go_to_x_regs() {
        let kinds = [ArgKind::Int, ArgKind::Int];
        let locs = classify_aapcs64_args_typed(&kinds);
        assert_eq!(locs[0], ArgLocation::Reg(X0));
        assert_eq!(locs[1], ArgLocation::Reg(X1));
    }

    #[test]
    fn typed_mixed_int_and_float_use_independent_banks() {
        // (int, float, int, float) → (X0, D0, X1, D1)
        let kinds = [ArgKind::Int, ArgKind::Float, ArgKind::Int, ArgKind::Float];
        let locs = classify_aapcs64_args_typed(&kinds);
        assert_eq!(locs[0], ArgLocation::Reg(X0));
        assert_eq!(locs[1], ArgLocation::Reg(D0));
        assert_eq!(locs[2], ArgLocation::Reg(X1));
        assert_eq!(locs[3], ArgLocation::Reg(D1));
    }

    #[test]
    fn typed_eight_float_args_all_in_regs() {
        let kinds = [ArgKind::Float; 8];
        let locs = classify_aapcs64_args_typed(&kinds);
        assert_eq!(locs[0], ArgLocation::Reg(D0));
        assert_eq!(locs[7], ArgLocation::Reg(D7));
    }

    #[test]
    fn typed_ninth_float_spills_to_stack() {
        let kinds = [ArgKind::Float; 9];
        let locs = classify_aapcs64_args_typed(&kinds);
        assert_eq!(locs[8], ArgLocation::Stack(0));
    }
}
