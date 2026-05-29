//! x86-64 Target Transform Info — Skylake and Zen4 cost tables.
//!
//! Provides reciprocal-throughput (`instruction_cost`) and RAW-latency
//! (`instruction_latency`) estimates for x86-64 machine opcodes.
//! Reference: Intel/AMD architecture optimization manuals and
//! Agner Fog's instruction tables (Skylake, Zen 4).

use crate::instructions::*;
use llvm_codegen::isel::MOpcode;
use llvm_codegen::schedule::x86_latency;
use llvm_codegen::tti::TargetTransformInfo;

// ── Microarchitecture profile ─────────────────────────────────────────────

/// x86-64 microarchitecture profile for TTI cost queries.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum X86Profile {
    /// Intel Skylake / Cascade Lake approximation (default).
    Skylake,
    /// AMD Zen 4 approximation.
    Zen4,
    /// Conservative generic fallback (same as Skylake for unknown ops).
    Generic,
}

// ── X86Tti ────────────────────────────────────────────────────────────────

/// x86-64 Target Transform Info.
///
/// Exposes per-opcode reciprocal-throughput and latency estimates for the
/// optimizer.  Use `X86Tti { profile: X86Profile::Skylake }` for the default
/// Intel tuning, or `X86Profile::Zen4` for AMD Zen 4.
pub struct X86Tti {
    /// Microarchitecture profile that controls per-opcode costs.
    pub profile: X86Profile,
}

impl Default for X86Tti {
    fn default() -> Self {
        Self {
            profile: X86Profile::Skylake,
        }
    }
}

impl TargetTransformInfo for X86Tti {
    /// Reciprocal throughput (approximate cycles, lower = faster).
    ///
    /// Sources: Agner Fog's instruction tables for Skylake/Zen4.
    fn instruction_cost(&self, opcode: MOpcode) -> u32 {
        match opcode {
            // ── data movement ──
            MOV_RR | MOV_RI | MOV_PR | MOVSX_32 | MOVSX_8 | MOVSX_16 | MOVZX_8 => 1,

            // ── integer arithmetic ──
            ADD_RR | ADD_RI | SUB_RR | SUB_RI | NEG_R | CQO => 1,
            IMUL_RR | IMUL_RRI => match self.profile {
                X86Profile::Skylake | X86Profile::Generic => 3,
                X86Profile::Zen4 => 3,
            },
            IDIV_R | DIV_R => match self.profile {
                X86Profile::Skylake | X86Profile::Generic => 20,
                X86Profile::Zen4 => 15,
            },

            // ── bitwise ──
            AND_RR | AND_RI | OR_RR | OR_RI | XOR_RR | XOR_RI | NOT_R => 1,

            // ── shifts ──
            SHL_RR | SHL_RI | SHR_RR | SHR_RI | SAR_RR | SAR_RI => 1,

            // ── comparisons ──
            CMP_RR | CMP_RI | TEST_RR | SETCC => 1,

            // ── control flow ──
            JMP | JCC | CALL_DIRECT | CALL_R | RET => 1,

            // ── stack ──
            PUSH_R | POP_R => 1,

            // ── misc ──
            NOP | LEA_RI | INLINE_ASM => 1,

            // ── spill loads/stores (L1-hit latency) ──
            MOV_LOAD_MR | MOV_STORE_RM => 1,

            // ── SIMD integer ──
            PADDD_RR | PSUBD_RR => 1,
            PMULLD_RR => match self.profile {
                X86Profile::Skylake | X86Profile::Generic => 5,
                X86Profile::Zen4 => 3,
            },
            ADDPS_RR | ADDPD_RR => match self.profile {
                X86Profile::Skylake | X86Profile::Generic => 1,
                X86Profile::Zen4 => 1,
            },
            MULPS_RR | MULPD_RR => match self.profile {
                X86Profile::Skylake | X86Profile::Generic => 1,
                X86Profile::Zen4 => 1,
            },
            DIVPS_RR => match self.profile {
                X86Profile::Skylake | X86Profile::Generic => 10,
                X86Profile::Zen4 => 8,
            },
            MOVAPS_RR | MOVDQU_LOAD_MR | MOVDQU_STORE_RM | MOVAPS_LOAD_MR => 1,

            // ── non-promotable frame access ──
            LEA_FRAME_MR | MOV_LOAD_REG_MR | MOV_STORE_REG_RM => 1,

            // ── atomics (fence-like cost) ──
            MFENCE | LOCK_CMPXCHG_MR | LOCK_XADD_MR | LOCK_XADD32_MR | XCHG_MR | LOCK_AND_MR
            | LOCK_OR_MR | LOCK_XOR_MR => 20,

            // ── SSE2 scalar double ──
            ADDSD_RR | SUBSD_RR => match self.profile {
                X86Profile::Skylake | X86Profile::Generic => 1,
                X86Profile::Zen4 => 1,
            },
            MULSD_RR => match self.profile {
                X86Profile::Skylake | X86Profile::Generic => 1,
                X86Profile::Zen4 => 1,
            },
            DIVSD_RR => match self.profile {
                X86Profile::Skylake | X86Profile::Generic => 10,
                X86Profile::Zen4 => 8,
            },
            SQRTSD_R => match self.profile {
                X86Profile::Skylake | X86Profile::Generic => 10,
                X86Profile::Zen4 => 8,
            },
            UCOMISD_RR | MOVSD_LOAD_MR | MOVSD_STORE_RM => 1,

            // ── SSE2 scalar single ──
            ADDSS_RR | SUBSS_RR => match self.profile {
                X86Profile::Skylake | X86Profile::Generic => 1,
                X86Profile::Zen4 => 1,
            },
            MULSS_RR => match self.profile {
                X86Profile::Skylake | X86Profile::Generic => 1,
                X86Profile::Zen4 => 1,
            },
            DIVSS_RR => match self.profile {
                X86Profile::Skylake | X86Profile::Generic => 10,
                X86Profile::Zen4 => 8,
            },
            SQRTSS_R => match self.profile {
                X86Profile::Skylake | X86Profile::Generic => 10,
                X86Profile::Zen4 => 8,
            },
            UCOMISS_RR | MOVSS_LOAD_MR | MOVSS_STORE_RM => 1,

            // ── FP ↔ integer conversions ──
            CVTTSD2SI_RR | CVTSI2SD_RR | CVTTSS2SI_RR | CVTSI2SS_RR | CVTSD2SS_RR | CVTSS2SD_RR
            | MOVAPD_RR | MOVAPD_RR_F32 => 1,

            // Default: conservatively 1 cycle.
            _ => 1,
        }
    }

    /// RAW latency in cycles.
    ///
    /// Delegates to the existing `x86_latency` table in `schedule.rs` which
    /// contains Skylake-calibrated latency values.
    fn instruction_latency(&self, opcode: MOpcode) -> u32 {
        x86_latency(opcode)
    }

    /// Memory operation cost.
    ///
    /// Stores have throughput ≈1.  Loads cost more for narrow/unaligned access.
    fn memory_op_cost(&self, is_store: bool, align: u32) -> u32 {
        if is_store {
            return 1;
        }
        match align {
            a if a >= 8 => 1,
            4 => 1,
            2 => 2,
            _ => 4,
        }
    }

    /// Recommended SIMD vector factor for AVX2 (256-bit) baseline.
    fn vector_factor(&self, scalar_bits: u32) -> u32 {
        match scalar_bits {
            16 => 16, // 16x i16 in 256 bits (AVX2)
            32 => 8,  // 8x i32 / f32 in 256 bits (AVX2)
            64 => 4,  // 4x i64 / f64 in 256 bits (AVX2)
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
    /// Requires at least 2 ops and an op cost ≤ 8 cycles (filters out
    /// division-heavy chains that are unlikely to benefit from SLP).
    fn slp_profitable(&self, scalar_count: usize, op_cost: u32) -> bool {
        scalar_count >= 2 && op_cost <= 8
    }
}

// ── tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skylake_div_cost_high() {
        let tti = X86Tti {
            profile: X86Profile::Skylake,
        };
        assert!(
            tti.instruction_cost(IDIV_R) >= 10,
            "IDIV should have high throughput cost"
        );
    }

    #[test]
    fn skylake_mov_cost_low() {
        let tti = X86Tti {
            profile: X86Profile::Skylake,
        };
        assert_eq!(tti.instruction_cost(MOV_RR), 1);
    }

    #[test]
    fn skylake_add_cost_low() {
        let tti = X86Tti {
            profile: X86Profile::Skylake,
        };
        assert_eq!(tti.instruction_cost(ADD_RR), 1);
    }

    #[test]
    fn skylake_imul_cost() {
        let tti = X86Tti {
            profile: X86Profile::Skylake,
        };
        assert_eq!(tti.instruction_cost(IMUL_RR), 3);
    }

    #[test]
    fn skylake_fp_div_high() {
        let tti = X86Tti {
            profile: X86Profile::Skylake,
        };
        assert!(
            tti.instruction_cost(DIVSD_RR) >= 8,
            "DIVSD should have high throughput cost"
        );
    }

    #[test]
    fn skylake_fp_add_cost() {
        let tti = X86Tti {
            profile: X86Profile::Skylake,
        };
        assert_eq!(tti.instruction_cost(ADDSD_RR), 1);
    }

    #[test]
    fn skylake_vector_factor_i32() {
        let tti = X86Tti {
            profile: X86Profile::Skylake,
        };
        assert_eq!(tti.vector_factor(32), 8);
    }

    #[test]
    fn skylake_vector_factor_f64() {
        let tti = X86Tti {
            profile: X86Profile::Skylake,
        };
        assert_eq!(tti.vector_factor(64), 4);
    }

    #[test]
    fn skylake_vector_factor_f32() {
        let tti = X86Tti {
            profile: X86Profile::Skylake,
        };
        assert_eq!(tti.vector_factor(32), 8);
    }

    #[test]
    fn zen4_div_cheaper_than_skylake() {
        let skylake = X86Tti {
            profile: X86Profile::Skylake,
        };
        let zen4 = X86Tti {
            profile: X86Profile::Zen4,
        };
        // Zen4 has better integer division throughput.
        assert!(zen4.instruction_cost(IDIV_R) <= skylake.instruction_cost(IDIV_R));
    }

    #[test]
    fn zen4_fp_div_cheaper_than_skylake() {
        let skylake = X86Tti {
            profile: X86Profile::Skylake,
        };
        let zen4 = X86Tti {
            profile: X86Profile::Zen4,
        };
        assert!(zen4.instruction_cost(DIVSD_RR) <= skylake.instruction_cost(DIVSD_RR));
    }

    #[test]
    fn latency_reuses_schedule_table() {
        let tti = X86Tti {
            profile: X86Profile::Skylake,
        };
        // IMUL latency should be 3 per schedule.rs
        assert_eq!(tti.instruction_latency(IMUL_RR), 3);
        // IDIV latency should be 20
        assert_eq!(tti.instruction_latency(IDIV_R), 20);
    }

    #[test]
    fn memory_op_cost_aligned() {
        let tti = X86Tti {
            profile: X86Profile::Skylake,
        };
        assert_eq!(tti.memory_op_cost(false, 8), 1);
        assert_eq!(tti.memory_op_cost(true, 8), 1);
    }

    #[test]
    fn memory_op_cost_unaligned() {
        let tti = X86Tti {
            profile: X86Profile::Skylake,
        };
        assert!(tti.memory_op_cost(false, 1) >= 2);
    }

    #[test]
    fn unroll_small_body() {
        let tti = X86Tti {
            profile: X86Profile::Skylake,
        };
        assert!(tti.unroll_factor(4, None) >= 2);
    }

    #[test]
    fn unroll_large_body() {
        let tti = X86Tti {
            profile: X86Profile::Skylake,
        };
        assert_eq!(tti.unroll_factor(32, None), 1);
    }

    #[test]
    fn slp_profitable_enough_ops() {
        let tti = X86Tti {
            profile: X86Profile::Skylake,
        };
        assert!(tti.slp_profitable(4, 1));
        assert!(!tti.slp_profitable(1, 1));
    }

    #[test]
    fn slp_not_profitable_expensive_ops() {
        let tti = X86Tti {
            profile: X86Profile::Skylake,
        };
        // Division chains (cost > 8) are not profitable to SLP
        assert!(!tti.slp_profitable(4, 20));
    }

    #[test]
    fn atomic_cost_high() {
        let tti = X86Tti {
            profile: X86Profile::Skylake,
        };
        assert!(tti.instruction_cost(MFENCE) >= 10);
        assert!(tti.instruction_cost(LOCK_CMPXCHG_MR) >= 10);
    }
}
