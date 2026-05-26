//! Target Transform Info (TTI) — microarchitectural cost model for optimizer decisions.
//!
//! The `TargetTransformInfo` trait abstracts per-target cost information so that
//! target-independent passes (SLP vectorizer, LICM, loop unroller) can query
//! instruction costs without knowing which target they are running on.
//!
//! All cost values are in approximate cycles.  Throughput (reciprocal) is used
//! for `instruction_cost`; latency (RAW) for `instruction_latency`.

use crate::isel::MOpcode;

// ── Trait definition ──────────────────────────────────────────────────────

/// Target Transform Info — microarchitectural cost model for optimizer decisions.
///
/// All costs are in approximate cycles. Throughput (reciprocal) is used
/// for `instruction_cost`; latency (RAW) for `instruction_latency`.
pub trait TargetTransformInfo: Send + Sync {
    /// Reciprocal throughput (lower = faster) for the given machine opcode.
    fn instruction_cost(&self, opcode: MOpcode) -> u32;
    /// RAW latency in cycles for the given machine opcode.
    fn instruction_latency(&self, opcode: MOpcode) -> u32;
    /// Cost of a memory operation (load or store) given alignment in bytes.
    fn memory_op_cost(&self, is_store: bool, align: u32) -> u32;
    /// Recommended SIMD vectorization factor for `scalar_bits`-wide elements.
    /// Returns 1 if vectorization is not recommended.
    fn vector_factor(&self, scalar_bits: u32) -> u32;
    /// Recommended loop unroll count given body instruction count and optional trip count.
    fn unroll_factor(&self, body_instrs: usize, trip_count: Option<u64>) -> u32;
    /// Returns true if SLP-vectorizing `scalar_count` ops each costing `op_cost` is profitable.
    fn slp_profitable(&self, scalar_count: usize, op_cost: u32) -> bool;
}

// ── Generic (conservative fallback) TTI ──────────────────────────────────

/// Conservative fallback TTI — used when no target provides one.
///
/// All instruction costs default to 1. Vector factors follow a simple
/// width-doubling heuristic. Unrolling is capped at 4x for small loops.
pub struct GenericTti;

impl TargetTransformInfo for GenericTti {
    fn instruction_cost(&self, _opcode: MOpcode) -> u32 {
        1
    }

    fn instruction_latency(&self, _opcode: MOpcode) -> u32 {
        1
    }

    fn memory_op_cost(&self, _is_store: bool, _align: u32) -> u32 {
        1
    }

    fn vector_factor(&self, scalar_bits: u32) -> u32 {
        match scalar_bits {
            32 => 4,
            64 => 2,
            _ => 1,
        }
    }

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

    fn slp_profitable(&self, scalar_count: usize, _op_cost: u32) -> bool {
        scalar_count >= 2
    }
}

// ── tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generic_tti_cost_is_one() {
        let tti = GenericTti;
        // Any opcode → cost 1
        assert_eq!(tti.instruction_cost(MOpcode(0)), 1);
        assert_eq!(tti.instruction_cost(MOpcode(0xFF)), 1);
    }

    #[test]
    fn generic_tti_latency_is_one() {
        let tti = GenericTti;
        assert_eq!(tti.instruction_latency(MOpcode(0)), 1);
    }

    #[test]
    fn generic_tti_vector_factor_f64() {
        let tti = GenericTti;
        assert_eq!(tti.vector_factor(64), 2);
    }

    #[test]
    fn generic_tti_vector_factor_f32() {
        let tti = GenericTti;
        assert_eq!(tti.vector_factor(32), 4);
    }

    #[test]
    fn generic_tti_vector_factor_other() {
        let tti = GenericTti;
        assert_eq!(tti.vector_factor(8), 1);
        assert_eq!(tti.vector_factor(16), 1);
    }

    #[test]
    fn generic_tti_unroll_small_body() {
        let tti = GenericTti;
        assert!(tti.unroll_factor(4, None) >= 2);
        assert_eq!(tti.unroll_factor(4, None), 4);
    }

    #[test]
    fn generic_tti_unroll_medium_body() {
        let tti = GenericTti;
        assert_eq!(tti.unroll_factor(12, None), 2);
    }

    #[test]
    fn generic_tti_unroll_large_body() {
        let tti = GenericTti;
        assert_eq!(tti.unroll_factor(64, None), 1);
    }

    #[test]
    fn generic_tti_unroll_with_known_trip_count() {
        let tti = GenericTti;
        // Small known trip count → fully unroll
        assert_eq!(tti.unroll_factor(4, Some(6)), 6);
        // Large known trip count → fall through to body-size heuristic
        assert!(tti.unroll_factor(4, Some(100)) <= 4);
    }

    #[test]
    fn generic_tti_slp_profitable() {
        let tti = GenericTti;
        assert!(tti.slp_profitable(4, 1));
        assert!(tti.slp_profitable(2, 1));
        assert!(!tti.slp_profitable(1, 1));
        assert!(!tti.slp_profitable(0, 1));
    }

    #[test]
    fn generic_tti_memory_op_cost() {
        let tti = GenericTti;
        assert_eq!(tti.memory_op_cost(false, 8), 1);
        assert_eq!(tti.memory_op_cost(true, 8), 1);
    }
}
