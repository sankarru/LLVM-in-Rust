//! AArch64 Target Transform Info — Cortex-A55 and Cortex-A72 cost tables.
//!
//! Provides reciprocal-throughput (`instruction_cost`) and RAW-latency
//! (`instruction_latency`) estimates for AArch64 machine opcodes.
//! Reference: ARM Cortex-A55/A72 Software Optimization Guides and
//! Agner Fog's instruction tables.

use crate::instructions::*;
use llvm_codegen::isel::MOpcode;
use llvm_codegen::tti::TargetTransformInfo;

// ── Microarchitecture profile ─────────────────────────────────────────────

/// AArch64 microarchitecture profile for TTI cost queries.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AArch64Profile {
    /// ARM Cortex-A55 (low-power, in-order, ARMv8.2).
    CortexA55,
    /// ARM Cortex-A72 (high-performance, out-of-order, ARMv8.0).
    CortexA72,
    /// Conservative generic fallback.
    Generic,
}

// ── AArch64Tti ────────────────────────────────────────────────────────────

/// AArch64 Target Transform Info.
///
/// Exposes per-opcode reciprocal-throughput and latency estimates for the
/// optimizer.  Use `AArch64Tti { profile: AArch64Profile::Generic }` for
/// a portable default, or select a core-specific profile for tuned output.
pub struct AArch64Tti {
    /// Microarchitecture profile that controls per-opcode costs.
    pub profile: AArch64Profile,
}

impl Default for AArch64Tti {
    fn default() -> Self {
        Self {
            profile: AArch64Profile::Generic,
        }
    }
}

impl TargetTransformInfo for AArch64Tti {
    /// Reciprocal throughput (approximate cycles, lower = faster).
    ///
    /// Sources: ARM Cortex-A55/A72 software optimization guides.
    fn instruction_cost(&self, opcode: MOpcode) -> u32 {
        match opcode {
            // ── data movement ──
            MOV_RR | MOV_IMM | MOV_WIDE | MOV_PR => 1,

            // ── integer arithmetic ──
            ADD_RR | SUB_RR | NEG_R => 1,
            MUL_RR => 3,
            SDIV_RR | UDIV_RR => match self.profile {
                AArch64Profile::CortexA55 => 8,
                AArch64Profile::CortexA72 => 10,
                AArch64Profile::Generic => 8,
            },

            // ── bitwise ──
            AND_RR | ORR_RR | EOR_RR | LSL_RR | LSR_RR | ASR_RR => 1,

            // ── comparisons ──
            CMP_RR | CSET => 1,

            // ── control flow ──
            B | B_COND | BL | BLR | RET => 1,

            // ── memory ──
            LDR | LDR_REG => match self.profile {
                AArch64Profile::CortexA55 => 3,
                AArch64Profile::CortexA72 => 4,
                AArch64Profile::Generic => 4,
            },
            STR | STR_REG => 1,
            LDR_FP | STR_FP => match self.profile {
                AArch64Profile::CortexA55 => 3,
                AArch64Profile::CortexA72 => 4,
                AArch64Profile::Generic => 4,
            },

            // ── sign extension ──
            SXTW | SXTB | SXTH => 1,

            // ── misc ──
            NOP | INLINE_ASM | SUB_FP_IMM => 1,

            // ── atomics ──
            DMB_ISH => 10,
            CASAL | LDADDAL | LDCLRAL | LDSETAL | LDEORAL | SWPAL => 20,
            LDXR | STXR => 5,

            // ── scalar FP arithmetic ──
            FADD_RR | FSUB_RR => match self.profile {
                AArch64Profile::CortexA55 => 2,
                AArch64Profile::CortexA72 => 9,
                AArch64Profile::Generic => 4,
            },
            FMUL_RR => match self.profile {
                AArch64Profile::CortexA55 => 4,
                AArch64Profile::CortexA72 => 9,
                AArch64Profile::Generic => 4,
            },
            FDIV_RR => match self.profile {
                AArch64Profile::CortexA55 => 14,
                AArch64Profile::CortexA72 => 32,
                AArch64Profile::Generic => 14,
            },
            FNEG_R | FMOV_RR => 1,
            FSQRT_R => match self.profile {
                AArch64Profile::CortexA55 => 14,
                AArch64Profile::CortexA72 => 32,
                AArch64Profile::Generic => 14,
            },
            FCMP_RR => 1,
            FCVTZS_RR | SCVTF_RR | UCVTF_RR => match self.profile {
                AArch64Profile::CortexA55 => 4,
                AArch64Profile::CortexA72 => 9,
                AArch64Profile::Generic => 4,
            },
            LDR_FP_SCALAR | STR_FP_SCALAR | MOVSD_LOAD_MR | MOVSD_STORE_RM => match self.profile {
                AArch64Profile::CortexA55 => 3,
                AArch64Profile::CortexA72 => 4,
                AArch64Profile::Generic => 4,
            },

            // Default: conservatively 1 cycle.
            _ => 1,
        }
    }

    /// RAW latency in cycles (approximated as instruction cost for AArch64).
    ///
    /// AArch64 cores generally have similar throughput and latency for
    /// most common operations.  For accurate latency tables per uarch,
    /// consult the ARM software optimization guides directly.
    fn instruction_latency(&self, opcode: MOpcode) -> u32 {
        // Use cost as a latency approximation — most AArch64 instructions
        // have similar throughput and latency values.
        self.instruction_cost(opcode)
    }

    /// Memory operation cost.
    fn memory_op_cost(&self, is_store: bool, align: u32) -> u32 {
        if is_store {
            1
        } else if align >= 8 {
            4
        } else {
            6
        }
    }

    /// Recommended SIMD vector factor for NEON (128-bit) baseline.
    fn vector_factor(&self, scalar_bits: u32) -> u32 {
        match scalar_bits {
            16 => 8,  // 8x i16 in 128 bits (NEON)
            32 => 4,  // 4x i32 / f32 in 128 bits (NEON)
            64 => 2,  // 2x i64 / f64 in 128 bits (NEON)
            _ => 1,
        }
    }

    /// Loop unroll recommendation.
    fn unroll_factor(&self, body_instrs: usize, trip_count: Option<u64>) -> u32 {
        if let Some(n) = trip_count {
            if n <= 8 {
                return n as u32;
            }
        }
        if body_instrs <= 8 {
            4
        } else if body_instrs <= 16 {
            2
        } else {
            1
        }
    }

    /// SLP profitability check.
    ///
    /// Requires at least 2 ops with an op cost ≤ 10 cycles.
    fn slp_profitable(&self, scalar_count: usize, op_cost: u32) -> bool {
        scalar_count >= 2 && op_cost <= 10
    }
}

// ── tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a55_mul_cost() {
        let tti = AArch64Tti {
            profile: AArch64Profile::CortexA55,
        };
        assert_eq!(tti.instruction_cost(MUL_RR), 3);
    }

    #[test]
    fn a72_sdiv_cost() {
        let tti = AArch64Tti {
            profile: AArch64Profile::CortexA72,
        };
        assert_eq!(tti.instruction_cost(SDIV_RR), 10);
    }

    #[test]
    fn a55_sdiv_cheaper_than_a72() {
        let a55 = AArch64Tti {
            profile: AArch64Profile::CortexA55,
        };
        let a72 = AArch64Tti {
            profile: AArch64Profile::CortexA72,
        };
        assert!(a55.instruction_cost(SDIV_RR) <= a72.instruction_cost(SDIV_RR));
    }

    #[test]
    fn a55_udiv_cost() {
        let tti = AArch64Tti {
            profile: AArch64Profile::CortexA55,
        };
        assert!(tti.instruction_cost(UDIV_RR) >= 4);
    }

    #[test]
    fn a72_fp_add_cost() {
        let tti = AArch64Tti {
            profile: AArch64Profile::CortexA72,
        };
        // A72 has higher FP throughput cost than A55 due to longer pipelines
        assert!(tti.instruction_cost(FADD_RR) >= 1);
    }

    #[test]
    fn a55_fp_div_expensive() {
        let tti = AArch64Tti {
            profile: AArch64Profile::CortexA55,
        };
        assert!(tti.instruction_cost(FDIV_RR) >= 10);
    }

    #[test]
    fn a72_fp_div_expensive() {
        let tti = AArch64Tti {
            profile: AArch64Profile::CortexA72,
        };
        assert!(tti.instruction_cost(FDIV_RR) >= 10);
    }

    #[test]
    fn aarch64_vector_factor_f32() {
        let tti = AArch64Tti {
            profile: AArch64Profile::Generic,
        };
        assert_eq!(tti.vector_factor(32), 4);
    }

    #[test]
    fn aarch64_vector_factor_f64() {
        let tti = AArch64Tti {
            profile: AArch64Profile::Generic,
        };
        assert_eq!(tti.vector_factor(64), 2);
    }

    #[test]
    fn aarch64_vector_factor_i16() {
        let tti = AArch64Tti {
            profile: AArch64Profile::Generic,
        };
        assert_eq!(tti.vector_factor(16), 8);
    }

    #[test]
    fn aarch64_unroll_small_body() {
        let tti = AArch64Tti {
            profile: AArch64Profile::Generic,
        };
        assert!(tti.unroll_factor(4, None) >= 2);
        assert_eq!(tti.unroll_factor(4, None), 4);
    }

    #[test]
    fn aarch64_unroll_large_body() {
        let tti = AArch64Tti {
            profile: AArch64Profile::Generic,
        };
        assert_eq!(tti.unroll_factor(32, None), 1);
    }

    #[test]
    fn aarch64_unroll_known_trip_count() {
        let tti = AArch64Tti {
            profile: AArch64Profile::Generic,
        };
        assert_eq!(tti.unroll_factor(4, Some(6)), 6);
    }

    #[test]
    fn aarch64_slp_profitable() {
        let tti = AArch64Tti {
            profile: AArch64Profile::Generic,
        };
        assert!(tti.slp_profitable(4, 5));
        assert!(tti.slp_profitable(2, 10));
        assert!(!tti.slp_profitable(1, 5));
        assert!(!tti.slp_profitable(4, 20));
    }

    #[test]
    fn aarch64_mov_cost_low() {
        let tti = AArch64Tti {
            profile: AArch64Profile::Generic,
        };
        assert_eq!(tti.instruction_cost(MOV_RR), 1);
        assert_eq!(tti.instruction_cost(MOV_IMM), 1);
        assert_eq!(tti.instruction_cost(MOV_WIDE), 1);
    }

    #[test]
    fn aarch64_add_cost_low() {
        let tti = AArch64Tti {
            profile: AArch64Profile::Generic,
        };
        assert_eq!(tti.instruction_cost(ADD_RR), 1);
        assert_eq!(tti.instruction_cost(SUB_RR), 1);
    }

    #[test]
    fn aarch64_memory_op_cost_aligned() {
        let tti = AArch64Tti {
            profile: AArch64Profile::Generic,
        };
        assert!(tti.memory_op_cost(false, 8) < tti.memory_op_cost(false, 1));
        assert_eq!(tti.memory_op_cost(true, 8), 1);
    }

    #[test]
    fn aarch64_latency_approx_cost() {
        let tti = AArch64Tti {
            profile: AArch64Profile::CortexA55,
        };
        // Latency should equal cost for AArch64 (approximation)
        assert_eq!(
            tti.instruction_latency(MUL_RR),
            tti.instruction_cost(MUL_RR)
        );
        assert_eq!(
            tti.instruction_latency(SDIV_RR),
            tti.instruction_cost(SDIV_RR)
        );
    }

    #[test]
    fn aarch64_atomic_cost_high() {
        let tti = AArch64Tti {
            profile: AArch64Profile::Generic,
        };
        assert!(tti.instruction_cost(CASAL) >= 10);
        assert!(tti.instruction_cost(DMB_ISH) >= 5);
    }
}
