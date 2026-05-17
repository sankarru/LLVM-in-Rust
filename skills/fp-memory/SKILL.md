---
name: fp-memory
description: Implement Milestone I sub-issues — floating-point arithmetic lowering (SSE2/NEON/RV F+D extensions, FP register class, FP calling convention) and non-promotable memory (stack frame, SP-relative alloca/load/store, non-constant GEP) across all backends.
---

# FP & Memory Lowering

Use this skill to implement individual Milestone I sub-issues end-to-end.

## Context

All backends currently stub FP arithmetic (emit 0) and non-promotable memory
(emit NOP).  The regalloc does not distinguish integer vs. FP register classes.
The infrastructure change needed first is a minimal register-class extension,
then each backend gets its own PR.

## Prerequisite: Register Class Infrastructure

Before per-backend FP work, `llvm-codegen` needs:

- `RegClass` enum: `Int` and `Float` variants (add to `isel.rs`)
- `VReg` carries a `RegClass` field: `VReg { id: u32, class: RegClass }`
- `MachineFunction::allocatable_fp_pregs: Vec<PReg>` alongside the existing
  `allocatable_pregs` (integer pool)
- `allocate_registers` dispatches to two separate linear-scan passes, one per
  class, producing a combined `RegAllocResult`
- Backends set `allocatable_fp_pregs` in `lower_function`

This infrastructure PR is the dependency for all FP sub-issues.
Branch: `feat/milestone-i-regclass-infra`

## x86_64 FP (after infra PR merges)

### New MOpcode constants (`instructions.rs`)
```
ADDSD_RR, SUBSD_RR, MULSD_RR, DIVSD_RR, SQRTSD_R
ADDSS_RR, SUBSS_RR, MULSS_RR, DIVSS_RR, SQRTSS_R
XORPD_RR  (for FNeg: xor with sign-bit mask)
UCOMISD_RR, UCOMISS_RR
CVTTSD2SI_RR, CVTSI2SD_RR, CVTTSS2SI_RR, CVTSI2SS_RR
MOVSD_LOAD_MR, MOVSD_STORE_RM   (FP spill/reload)
MOVSS_LOAD_MR, MOVSS_STORE_RM
```

### New FP registers (`regs.rs`)
```rust
pub const XMM0: PReg = PReg(16);   // through
pub const XMM15: PReg = PReg(31);
pub const FP_ALLOCATABLE: [PReg; 8] = [XMM0..XMM7]; // caller-saved
pub const FP_CALLEE_SAVED: [PReg; 8] = [XMM8..XMM15]; // Win64 only
```

### Calling convention (`abi.rs`)
- SysV: float/double args → XMM0..XMM7; return → XMM0
- Win64: float/double args → XMM0..XMM3; return → XMM0
- Extend `ArgLocation` with `FpReg(PReg)`

### Lowering (`lower.rs`)
- `InstrKind::FAdd` with float ty → emit `ADDSS_RR dst, lhs, rhs`
- `InstrKind::FAdd` with double ty → emit `ADDSD_RR dst, lhs, rhs`
- Same pattern for FSub/FMul/FDiv/FRem/FNeg/FCmp
- FNeg: emit `XORPD_RR dst, src, sign_mask_constant`
- All FP VRegs use `RegClass::Float`

### Encoding (`encode.rs`)
- SSE2 two-byte prefix `0x66` + `0x0F` + opcode byte
- e.g. ADDSD: `66 0F 58 /r`
- ModRM uses XMM register numbers (0–7 for XMM0–7)

### Tests
- At least 15 encoding tests in `tests/fp_encode.rs`
- At least 10 differential fixture files: `fixtures/fp_add_f64.ll`, etc.
- Smoke test: compile `double add(double a, double b) { return a + b; }` IR,
  link, call with `(1.5, 2.5)`, assert result is `4.0`

## AArch64 FP (after infra PR merges)

### New MOpcode constants (`instructions.rs`)
```
FADD_RR, FSUB_RR, FMUL_RR, FDIV_RR, FNEG_R, FSQRT_R
FCMP_RR   (sets flags; pair with FCSEL or B_COND)
FMOV_RR, FMOV_IMM
FCVTZS_RR, SCVTF_RR, UCVTF_RR   (FP ↔ integer)
LDR_FP_SCALAR, STR_FP_SCALAR     (FP spill/reload, distinct from existing LDR_FP)
```

### New FP registers (`regs.rs`)
AArch64 hardware shares the 32 V-registers between SIMD and scalar FP.
```rust
pub const D0: PReg = PReg(32);   // through D31 = PReg(63)
pub const FP_ALLOCATABLE: [PReg; 16] = [D0..D7, D16..D23]; // caller-saved
pub const FP_CALLEE_SAVED: [PReg; 8] = [D8..D15];
```

### Calling convention (`abi.rs`)
- AAPCS64: float/double args → V0..V7; return → V0
- Extend `classify_aapcs64_args` to emit `ArgLocation::FpReg` for float types

### Encoding (`encode.rs`)
- Scalar FP: 32-bit instruction words with `ftype` bits [23:22] selecting S/D
- FADD: `0x1E202800 | (ftype<<22) | (Rm<<16) | (Rn<<5) | Rd`
- FSUB: `0x1E203800 | ...`; FMUL: `0x1E200800 | ...`; FDIV: `0x1E201800 | ...`
- FNEG: `0x1E214000 | (ftype<<22) | (Rn<<5) | Rd`
- FCMP: `0x1E202000 | (ftype<<22) | (Rm<<16) | (Rn<<5)` (Rd=0)

### Tests
- At least 15 encoding tests in `tests/fp_encode.rs`
- Differential fixture: `double add(double, double)` matches clang AArch64 output

## RISC-V FP (after infra PR merges)

### New MOpcode constants (`instructions.rs`)
```
FADD_D, FSUB_D, FMUL_D, FDIV_D, FSQRT_D, FNEG_D
FADD_S, FSUB_S, FMUL_S, FDIV_S, FSQRT_S, FNEG_S
FMV_D_X, FMV_X_D, FMV_W_X, FMV_X_W   (int ↔ FP reg transfer)
FCVT_D_W, FCVT_W_D, FCVT_D_WU, FCVT_WU_D  (conversion)
FLD, FSD   (FP load/store)
```

### New FP registers (`regs.rs`)
```rust
pub const F0: PReg = PReg(32);   // through F31 = PReg(63)
pub const FP_ARG_REGS: [PReg; 8] = [F10..F17];   // fa0..fa7
pub const FP_ALLOCATABLE: [PReg; 12] = [F0..F7, F10..F17]; // ft0-7, fa0-7
pub const FP_CALLEE_SAVED: [PReg; 12] = [F8, F9, F18..F27]; // fs0-11
```

### Encoding (`encode.rs`)
- R-type: `funct7 | rs2 | rs1 | funct3 | rd | opcode`
- FADD.D: `funct7=0b0000001`, `opcode=0x53`
- Use `rm=0b111` (dynamic rounding mode) for all ops

### Tests
- 10+ encoding unit tests
- Differential fixture with clang RISC-V cross-compile oracle

## Non-Promotable Memory (all backends)

This PR is independent of the FP register class infra — can land first.

### What "non-promotable" means
`mem2reg` promotes `alloca`s that:
- Are only loaded/stored (no `getelementptr` with non-constant indices)
- Are not passed to other functions (do not escape)

When an `alloca` does NOT meet these criteria, the backend must:
1. Assign it a stack frame slot (byte offset from frame pointer)
2. Emit SP/FP-relative stores and loads

### Backend changes

**All backends — `lower.rs`**:
- Add `frame_slots: HashMap<VReg, i32>` to the lowering context
- On `Alloca`: call `assign_frame_slot(size, align)` → returns `i32` byte
  offset; map `dst_vreg → offset`
- On `Store` where `ptr_vreg` maps to a frame slot: emit SP-relative store
- On `Load` where `ptr_vreg` maps to a frame slot: emit SP-relative load
- On `GEP` with constant index over a frame-slotted base: add offset to slot

**x86_64 — stack frame**:
- Prologue: `SUB RSP, frame_size` (aligned to 16)
- `Store` to frame slot: `MOV [RBP-offset], src`
- `Load` from frame slot: `MOV dst, [RBP-offset]`
- New opcodes: `MOV_STORE_FP_RM` (store to frame), `MOV_LOAD_FP_MR` (load)

**AArch64 — stack frame**:
- Prologue: existing frame pointer setup; extend to reserve slots
- `STR src, [fp, #-offset]` / `LDR dst, [fp, #-offset]`

**RISC-V — stack frame**:
- Prologue: `ADDI sp, sp, -frame_size`; `SD ra, offset(sp)`
- `SD src, -offset(s0)` / `LD dst, -offset(s0)`

### Tests
- `alloca_escapes_to_callee`: alloca whose address is passed to an external
  function; callee writes a value; after return, load must return that value
- `alloca_in_loop`: alloca used across loop iterations (not promotable)
- `struct_field_store_load`: struct alloca with two field stores + loads

## Workflow for Each Sub-Issue

1. Branch from `origin/main`: `git fetch origin && git checkout -b feat/milestone-i-<slug> origin/main`
2. Implement in the target crate only (minimal diff)
3. `cargo test -p <crate>` green before committing
4. `cargo test --workspace` green before pushing
5. Open PR with body following the template in AGENTS.md
6. Self-review: read the diff with a code-reviewer mindset; open any issues found
7. Fix in same branch, push follow-up commits
8. Post `gh pr review --comment` summarizing findings
9. When CI is green and no open review findings remain: `gh pr merge <N> --squash`
10. `gh issue close <sub-issue-N>`
11. When all sub-issues done: `gh issue close 285`; update #93 Status Snapshot
