//! FMF-aware reassociation pass for floating-point instructions.
//!
//! This pass performs algebraic simplifications on FP instructions when the
//! corresponding `FastMathFlags` permit the transformation.  It does a single
//! RPO scan over all basic blocks and rewrites each qualifying instruction
//! in-place, returning `true` if any changes were made.
//!
//! ## Simplifications applied
//!
//! | Pattern              | Condition          | Result     |
//! |----------------------|--------------------|------------|
//! | `x + 0.0`            | `nsz` or `fast`    | `x`        |
//! | `0.0 + x`            | `nsz` or `fast`    | `x`        |
//! | `x - 0.0`            | `nsz` or `fast`    | `x`        |
//! | `x * 1.0`            | `nnan` or `fast`   | `x`        |
//! | `1.0 * x`            | `nnan` or `fast`   | `x`        |
//! | `x / 1.0`            | `nnan` or `fast`   | `x`        |
//! | `x * 0.0`            | `nnan+ninf` or `fast` | `0.0`   |
//! | `(x + C1) + C2`      | both `reassoc`     | `x + (C1+C2)` |
//!
//! The pass does **not** need def-use chains: it rewrites the instruction kind
//! of the current instruction, and the substitution map (InstrId → ValueRef)
//! propagates the identity into later users within the same scan (the same
//! technique used by `ConstProp`).
//!
//! Replaced instructions (where the result is an identity to one operand) are
//! removed from their block bodies after the scan, exactly like `ConstProp`
//! removes folded instructions.

use crate::pass::FunctionPass;
use llvm_ir::TypeData;
use llvm_ir::{ConstantData, Context, Function, InstrId, InstrKind, ValueRef};
use std::collections::HashMap;

/// FMF-aware reassociation pass.
pub struct ReassocPass;

impl FunctionPass for ReassocPass {
    fn name(&self) -> &'static str {
        "reassoc"
    }

    fn run_on_function(&mut self, ctx: &mut Context, func: &mut Function) -> bool {
        if func.blocks.is_empty() {
            return false;
        }

        // Map InstrId → its substitution (a ValueRef the instruction is equivalent to).
        let mut subst: HashMap<InstrId, ValueRef> = HashMap::new();
        // Map InstrId → its new (rewritten) InstrKind, for constant-chain folding.
        let mut rewritten: HashMap<InstrId, InstrKind> = HashMap::new();

        let block_order = crate::const_prop::rpo(func);

        for bi in block_order {
            let body: Vec<InstrId> = func.blocks[bi].body.clone();
            for iid in body {
                // Apply pending substitutions to operands first.
                let kind = if !subst.is_empty() {
                    apply_subst(func.instr(iid).kind.clone(), &subst)
                } else {
                    func.instr(iid).kind.clone()
                };

                // Attempt to simplify the (possibly updated) instruction.
                if let Some(result) = try_simplify_fp(ctx, func, &kind, &subst, &rewritten) {
                    match result {
                        SimplifyResult::Identity(vref) => {
                            // The instruction is equivalent to one of its operands.
                            subst.insert(iid, vref);
                            // Also update the kind so the block body removal works.
                            func.instr_mut(iid).kind = kind;
                        }
                        SimplifyResult::NewKind(new_kind) => {
                            // The instruction has a new form (e.g. combined constants).
                            func.instr_mut(iid).kind = new_kind.clone();
                            rewritten.insert(iid, new_kind);
                        }
                    }
                } else {
                    // No simplification — just commit the substituted kind.
                    func.instr_mut(iid).kind = kind;
                }
            }

            // Propagate substitutions into the terminator as well.
            if let Some(tid) = func.blocks[bi].terminator {
                if !subst.is_empty() {
                    let new_kind = apply_subst(func.instr(tid).kind.clone(), &subst);
                    func.instr_mut(tid).kind = new_kind;
                }
            }
        }

        if subst.is_empty() && rewritten.is_empty() {
            return false;
        }

        // Remove identity-substituted instructions from block bodies.
        for bb in &mut func.blocks {
            bb.body.retain(|id| !subst.contains_key(id));
        }
        true
    }
}

// ---------------------------------------------------------------------------
// Simplification result
// ---------------------------------------------------------------------------

enum SimplifyResult {
    /// The instruction is identical to an existing value; remove it.
    Identity(ValueRef),
    /// The instruction should keep its slot but with a different kind.
    NewKind(InstrKind),
}

// ---------------------------------------------------------------------------
// Core simplification logic
// ---------------------------------------------------------------------------

fn try_simplify_fp(
    ctx: &mut Context,
    func: &Function,
    kind: &InstrKind,
    subst: &HashMap<InstrId, ValueRef>,
    rewritten: &HashMap<InstrId, InstrKind>,
) -> Option<SimplifyResult> {
    match kind {
        InstrKind::FAdd { flags, lhs, rhs } => {
            let nsz_or_fast = flags.nsz || flags.fast;
            let ty = instr_type(ctx, func, *lhs)?;

            // x + 0.0  →  x  (nsz or fast)
            if nsz_or_fast {
                if is_fp_zero(ctx, *rhs) {
                    return Some(SimplifyResult::Identity(*lhs));
                }
                // 0.0 + x  →  x
                if is_fp_zero(ctx, *lhs) {
                    return Some(SimplifyResult::Identity(*rhs));
                }
            }

            // Constant-chain: (x + C1) + C2  →  x + (C1+C2)   when both have reassoc
            let reassoc_or_fast = flags.reassoc || flags.fast;
            if reassoc_or_fast {
                if let Some(c2) = fp_constant_bits(ctx, *rhs) {
                    let inner_lhs = resolve(*lhs, subst);
                    if let Some((
                        InstrKind::FAdd {
                            flags: f2,
                            lhs: x,
                            rhs: rhs2,
                        },
                        _inner_lhs_id,
                    )) = get_instr_kind_by_ref(func, rewritten, inner_lhs)
                    {
                        if f2.reassoc || f2.fast {
                            if let Some(c1) = fp_constant_bits(ctx, *rhs2) {
                                let combined = f64::from_bits(c1) + f64::from_bits(c2);
                                if combined.is_finite() {
                                    let new_c = ctx.const_float(ty, combined.to_bits());
                                    let new_kind = InstrKind::FAdd {
                                        flags: *flags,
                                        lhs: *x,
                                        rhs: ValueRef::Constant(new_c),
                                    };
                                    return Some(SimplifyResult::NewKind(new_kind));
                                }
                            }
                        }
                    }
                }
            }

            None
        }

        InstrKind::FSub { flags, lhs, rhs } => {
            let nsz_or_fast = flags.nsz || flags.fast;
            // x - 0.0  →  x  (nsz or fast)
            if nsz_or_fast && is_fp_zero(ctx, *rhs) {
                return Some(SimplifyResult::Identity(*lhs));
            }
            None
        }

        InstrKind::FMul { flags, lhs, rhs } => {
            let nnan_or_fast = flags.nnan || flags.fast;
            let ty = instr_type(ctx, func, *lhs)?;

            // x * 1.0  →  x  (nnan or fast)
            if nnan_or_fast {
                if is_fp_one(ctx, *rhs) {
                    return Some(SimplifyResult::Identity(*lhs));
                }
                // 1.0 * x  →  x
                if is_fp_one(ctx, *lhs) {
                    return Some(SimplifyResult::Identity(*rhs));
                }
            }

            // x * 0.0  →  0.0  (nnan + ninf, or fast)
            let nnan_ninf_or_fast = (flags.nnan && flags.ninf) || flags.fast;
            if nnan_ninf_or_fast && (is_fp_zero(ctx, *rhs) || is_fp_zero(ctx, *lhs)) {
                let zero_cid = ctx.const_float(ty, 0f64.to_bits());
                return Some(SimplifyResult::Identity(ValueRef::Constant(zero_cid)));
            }

            None
        }

        InstrKind::FDiv { flags, lhs, rhs } => {
            let nnan_or_fast = flags.nnan || flags.fast;
            // x / 1.0  →  x  (nnan or fast)
            if nnan_or_fast && is_fp_one(ctx, *rhs) {
                return Some(SimplifyResult::Identity(*lhs));
            }
            None
        }

        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Helper utilities
// ---------------------------------------------------------------------------

/// Resolve a ValueRef through the substitution map.
fn resolve(vref: ValueRef, subst: &HashMap<InstrId, ValueRef>) -> ValueRef {
    if let ValueRef::Instruction(id) = vref {
        subst.get(&id).copied().unwrap_or(vref)
    } else {
        vref
    }
}

/// Look up the current InstrKind for an instruction ValueRef, consulting the
/// `rewritten` map first (for already-rewritten instructions) then the function
/// instruction pool.  Returns `None` if vref is not an instruction.
fn get_instr_kind_by_ref<'a>(
    func: &'a Function,
    rewritten: &'a HashMap<InstrId, InstrKind>,
    vref: ValueRef,
) -> Option<(&'a InstrKind, InstrId)> {
    if let ValueRef::Instruction(id) = vref {
        let kind = rewritten.get(&id).unwrap_or(&func.instr(id).kind);
        Some((kind, id))
    } else {
        None
    }
}

/// Return true if `vref` is a constant `+0.0` or `-0.0` FP zero.
fn is_fp_zero(ctx: &Context, vref: ValueRef) -> bool {
    if let ValueRef::Constant(cid) = vref {
        match ctx.get_const(cid) {
            ConstantData::Float { bits, .. } => {
                let f = f64::from_bits(*bits);
                f == 0.0
            }
            ConstantData::ZeroInitializer(_) => true,
            _ => false,
        }
    } else {
        false
    }
}

/// Return true if `vref` is a constant `1.0` (f32 or f64).
fn is_fp_one(ctx: &Context, vref: ValueRef) -> bool {
    if let ValueRef::Constant(cid) = vref {
        match ctx.get_const(cid) {
            ConstantData::Float { ty, bits } => match ctx.get_type(*ty) {
                TypeData::Float(llvm_ir::FloatKind::Single) => {
                    f32::from_bits(*bits as u32) == 1.0f32
                }
                TypeData::Float(llvm_ir::FloatKind::Double) => f64::from_bits(*bits) == 1.0f64,
                _ => false,
            },
            _ => false,
        }
    } else {
        false
    }
}

/// Return the raw bits of a float constant (always as f64 bits regardless of
/// source width — we promote f32 to f64 for the arithmetic).
fn fp_constant_bits(ctx: &Context, vref: ValueRef) -> Option<u64> {
    if let ValueRef::Constant(cid) = vref {
        match ctx.get_const(cid) {
            ConstantData::Float { ty, bits } => match ctx.get_type(*ty) {
                TypeData::Float(llvm_ir::FloatKind::Single) => {
                    let f = f32::from_bits(*bits as u32) as f64;
                    Some(f.to_bits())
                }
                TypeData::Float(llvm_ir::FloatKind::Double) => Some(*bits),
                _ => None,
            },
            _ => None,
        }
    } else {
        None
    }
}

/// Return the type of a ValueRef within the function.
fn instr_type(ctx: &Context, func: &Function, vref: ValueRef) -> Option<llvm_ir::TypeId> {
    match vref {
        ValueRef::Instruction(id) => Some(func.instr(id).ty),
        ValueRef::Argument(id) => Some(func.arg(id).ty),
        ValueRef::Constant(id) => Some(ctx.type_of_const(id)),
        ValueRef::Global(_) => Some(ctx.ptr_ty),
    }
}

/// Apply a substitution map to the operands of an InstrKind.
/// Only ValueRef::Instruction references that appear in `subst` are replaced.
fn apply_subst(kind: InstrKind, subst: &HashMap<InstrId, ValueRef>) -> InstrKind {
    let s = |v: ValueRef| -> ValueRef {
        if let ValueRef::Instruction(id) = v {
            subst.get(&id).copied().unwrap_or(v)
        } else {
            v
        }
    };
    match kind {
        InstrKind::FAdd { flags, lhs, rhs } => InstrKind::FAdd {
            flags,
            lhs: s(lhs),
            rhs: s(rhs),
        },
        InstrKind::FSub { flags, lhs, rhs } => InstrKind::FSub {
            flags,
            lhs: s(lhs),
            rhs: s(rhs),
        },
        InstrKind::FMul { flags, lhs, rhs } => InstrKind::FMul {
            flags,
            lhs: s(lhs),
            rhs: s(rhs),
        },
        InstrKind::FDiv { flags, lhs, rhs } => InstrKind::FDiv {
            flags,
            lhs: s(lhs),
            rhs: s(rhs),
        },
        InstrKind::Ret { val } => InstrKind::Ret { val: val.map(s) },
        InstrKind::CondBr {
            cond,
            then_dest,
            else_dest,
        } => InstrKind::CondBr {
            cond: s(cond),
            then_dest,
            else_dest,
        },
        // For all other kinds, pass through unchanged (the reassoc pass only
        // transforms FP arithmetic; other kinds are handled by ConstProp).
        other => other,
    }
}
