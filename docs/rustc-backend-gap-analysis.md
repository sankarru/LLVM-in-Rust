# rustc Codegen Backend Gap Analysis

This document records the gap between the current LLVM-in-Rust pipeline and a
production-grade `rustc_codegen_ssa` backend, capturing every blocking item and
the estimated effort to close it.

## Background

`rustc` exposes `rustc_codegen_ssa::traits::CodegenBackend` as a trait that can
be implemented by external codegen crates (loaded at runtime via
`-Zcodegen-backend=<path>`).  The trait is gated behind
`#![feature(rustc_private)]`, which is **nightly-only**.  Implementing it on
stable requires either waiting for stabilisation or shipping a proc-macro shim
that generates the trait impl at nightly compile time and ships pre-compiled.

The `llvm-in-rust-rustc-backend` crate ships stable-compatible shim tests
(see `src/shim.rs`) that exercise the full IR→object pipeline and will serve as
regression guards once the nightly wiring is added.

---

## Required trait surface

| Trait method | Status | Notes |
|---|---|---|
| `CodegenBackend::init` | Not implemented | Setup hook; trivial to stub |
| `CodegenBackend::print` | Not implemented | Diagnostic printing; can be no-op |
| `CodegenBackend::codegen_crate` | Not implemented | **Core method** — produces `Box<dyn Any>` (ongoing CGUs) |
| `CodegenBackend::join_codegen` | Not implemented | Waits for background CGU work, collects `ObjectFile`s |
| `CodegenBackend::link` | Not implemented | Passes object files to `rustc`'s linker |
| `WriteBackendMethods` | Not implemented | Per-CGU serialisation hooks |
| `ModuleLlvm` (or equivalent) | Not needed | We use our own `ObjectFile` |

### Dependency gating

All trait items above live in the `rustc_codegen_ssa` and `rustc_middle` crates,
which are **not published to crates.io** and can only be used via `rustc_private`.
This requires:

```toml
[package]
# Cargo.toml
build = "build.rs"

[build-dependencies]
rustc_private = { optional = true }
```

And in the crate root:

```rust
#![feature(rustc_private)]
extern crate rustc_codegen_ssa;
extern crate rustc_middle;
```

---

## MIR → LLVM IR translation (largest gap)

The critical path between rustc and our backend is a MIR-to-LLVM-IR translation
layer.  rustc's MIR is a control-flow graph of `BasicBlock`s with `Statement`s
and `Terminator`s.  Our IR (`llvm-ir`) uses a structurally similar representation;
the translation is conceptually straightforward but requires handling every MIR
construct.

### Statement lowering

| MIR statement | IR lowering | Gap |
|---|---|---|
| `Assign(place, Rvalue::Use(operand))` | `store` / `mov` | Partial — place projection chains not implemented |
| `Assign(place, Rvalue::BinaryOp)` | `add` / `sub` / etc. | Straightforward; needs overflow flags |
| `Assign(place, Rvalue::UnaryOp)` | `neg` / `not` | Straightforward |
| `Assign(place, Rvalue::Ref)` | `alloca` + `load` address | Needs address-taken analysis |
| `Assign(place, Rvalue::Cast)` | `sext` / `zext` / `bitcast` | Partial |
| `Assign(place, Rvalue::Aggregate)` | struct/array construction | Not implemented |
| `StorageLive` / `StorageDead` | `alloca` lifetime hints | Can be ignored initially |
| `SetDiscriminant` | enum tag write | Not implemented |
| `Deinit` | poison / undef | Can be no-op initially |

### Terminator lowering

| MIR terminator | IR lowering | Gap |
|---|---|---|
| `Goto` | unconditional `br` | Trivial |
| `SwitchInt` | `switch` or chain of `br` | Needs integer comparison helpers |
| `Return` | `ret` | Trivial |
| `Call` | `call` + landing pad | Unwinding not yet supported |
| `Assert` | conditional `br` + abort | Needs `__rust_panic` linkage |
| `Drop` | call to drop glue | Requires drop elaboration |
| `InlineAsm` | `inline_asm` IR node | Partial (x86 inline asm parses) |
| `Unreachable` | `unreachable` | Trivial |

---

## Type mapping

| Rust / MIR type | IR type | Gap |
|---|---|---|
| `bool`, `u8`..`u64`, `i8`..`i64` | `i1`..`i64` | Straightforward |
| `f32`, `f64` | `float`, `double` | Supported |
| `*T`, `&T`, `Box<T>` | `ptr` (opaque) | Need pointer-size from target |
| `[T; N]` | `[T x N]` | Supported |
| `(A, B, ...)` | `{ A, B, ... }` | Supported |
| `struct`/`enum` with layout | `{ ... }` with field offsets | Needs `rustc_target::abi` |
| `dyn Trait` | fat pointer `{ *data, *vtable }` | Requires vtable layout |
| `impl Trait` | monomorphised concrete type | Handled by MIR monomorphisation |
| SIMD types | `<N x T>` | Partial (x86 SSE2 via VP intrinsics) |

---

## ABI / calling convention

rustc lowers calling conventions via `rustc_target::abi::call::FnAbi`.  The
`FnAbi` struct records how each argument and return value is passed (by value in
registers, by pointer, split across multiple registers, etc.).  Our ABI layer
(`llvm-target-x86/src/abi.rs`) implements SysV AMD64 and Win64 but only for the
simple register-or-stack cases.  Missing pieces:

- **Large struct decomposition** — structs > 2 eightbytes that must be passed by
  invisible reference (`PassMode::Indirect`).
- **Homogeneous float aggregates (HFA/HVA)** on AArch64 — packed into FP regs.
- **Variadic argument handling** — `va_start` / `va_arg` lowering.
- **C-unwind ABI** — `nounwind` vs `unwind` attribute propagation.

---

## Exception / unwinding support

rustc generates landing pads for `Drop` glue and `catch_unwind`.  Our IR has
`LandingPad` and `Invoke`/`Resume` nodes but the codegen backends do not yet
emit the EH frame sections (`.eh_frame` / `.pdata`) needed to unwind.

This is a significant gap; blocking items:

1. `llvm-codegen`: emit `.eh_frame` (DWARF CIE + FDE) in ELF and Mach-O.
2. `llvm-target-x86`: record frame layout (push/pop, sub rsp) in `EncodeCtx` for FDE.
3. `llvm-target-arm`: equivalent for AArch64 DWARF unwind.
4. COFF: emit `.pdata` + `.xdata` (Win64 structured exception handling).

---

## Incremental compilation / CGU splitting

rustc splits compilation into Compilation Units (CGUs) for incremental rebuilds.
Each CGU maps to one `Module` in our IR.  The
`CodegenBackend::codegen_crate` method receives an iterator of CGUs and may
process them in parallel.

Our `PassManager` and backends are single-threaded; adding `Send + Sync` bounds
and parallelising per-CGU work is straightforward once the trait wiring exists.

---

## Debug info

rustc emits DWARF debug info via `rustc_codegen_ssa::mir::debuginfo`.  Our
`emit.rs` has a `DebugLineRow` stub in `Section` but does not yet emit
`.debug_info`, `.debug_abbrev`, or `.debug_line` sections.

---

## Estimated effort to production

| Area | Effort |
|---|---|
| Nightly gating + trait stubs | 1–2 days |
| MIR→IR translation (core ops) | 2–3 weeks |
| ABI completeness | 1 week |
| Unwinding / EH frames | 2–3 weeks |
| Debug info | 1–2 weeks |
| CGU parallelism | 2–3 days |
| End-to-end `hello_world` | 1–2 weeks (integration) |

Total to compile a simple `fn main()` end-to-end through `cargo build`: **~6–8 weeks**.

---

## Next steps

1. Gate `--features rustc-backend` behind a nightly CI job.
2. Add `extern crate rustc_codegen_ssa` stub in `src/codegen_backend.rs`.
3. Implement the `CodegenBackend` trait with `unimplemented!()` stubs.
4. Implement MIR→IR translation for `Assign` + `Goto` + `Return` (enough for
   `fn add(a: i32, b: i32) -> i32 { a + b }`).
5. Wire up to `cargo` via `-Zcodegen-backend` and verify the stub compiles a
   trivial crate without panicking.
