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
which are **not published to crates.io** and can only be accessed via
`rustc_private`.  There is no Cargo dependency entry — the crates are provided by
the `rustc-dev` toolchain component.  Add to the crate root:

```rust
// lib.rs (nightly only, requires rustc-dev component)
#![feature(rustc_private)]
extern crate rustc_codegen_ssa;
extern crate rustc_middle;
extern crate rustc_target;
```

No `Cargo.toml` changes are needed for the `extern crate` lines above; the
`rustc-dev` component makes these crates available on the sysroot.  The
`--features rustc-backend` flag in this workspace is purely a compile-time guard
that keeps the nightly-only code out of stable builds.

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
| `cdylib`/`staticlib` linker support | 3–5 days |
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

---

## CI Modes

The rustc-backend crate ships two operational modes:

- **Stable shim mode** — runs on every push/PR via `cargo test -p llvm-in-rust-rustc-backend`.
  The 13 shim tests exercise the full IR→object pipeline through raw IR strings with no
  `rustc_private` dependency.  These always pass.

- **Nightly backend mode** — runs in `.github/workflows/rustc-backend-nightly.yml`
  (`continue-on-error: true`).  Uses `dtolnay/rust-toolchain@nightly` with the `rustc-dev`
  and `llvm-tools` components, then attempts `cargo +nightly build` of
  `bench/rustc-backend-validation/` with `-Z codegen-backend=llvm-in-rust-backend`.
  This lane is non-blocking until all Milestone K items are merged.

---

## MIR Construct Status Tables

These tables track which MIR constructs the nightly backend currently handles.
Status values: **Working** (produces correct output), **Partial** (compiles but incomplete),
**Stub** (accepted without error but no real codegen), **Not Started** (unimplemented/panics).

The shim tests cover `Return` (every shim) and `Goto` (conditional-branch shim), so those
two terminators are promoted above "Not Started".

### MIR Terminators

| Terminator Kind     | Status      | Notes |
|---------------------|-------------|-------|
| `Return`            | Working     | Every shim test exercises this path |
| `Goto`              | Partial     | Unconditional branch emitted; phi-copies not yet wired |
| `SwitchInt`         | Not Started | Needs `switch` or br-chain lowering |
| `Call`              | Not Started | Requires ABI classification + landing-pad support |
| `Drop`              | Not Started | Requires drop-glue elaboration |
| `DropAndReplace`    | Not Started | Deprecated in recent MIR; equivalent to Drop + assign |
| `Assert`            | Not Started | Needs conditional br + `__rust_panic` linkage |
| `Resume`            | Not Started | Unwind resume; blocked on EH frame emission |
| `Abort`             | Not Started | Calls `core::panicking::panic_explicit` |
| `Unreachable`       | Not Started | `unreachable` IR node; trivial once wired |
| `Yield`             | Not Started | Generator / coroutine support not planned for Milestone K |
| `FalseEdge`         | Not Started | MIR-only borrow-check artifact; drops before codegen |
| `FalseUnwind`       | Not Started | MIR-only borrow-check artifact; drops before codegen |
| `InlineAsm`         | Not Started | x86 inline-asm parser exists; rustc AsmOperand not wired |
| `GeneratorDrop`     | Not Started | Generator-specific; not planned for Milestone K |

### MIR Rvalues

| Rvalue Kind          | Status      | Notes |
|----------------------|-------------|-------|
| `Use`                | Not Started | Simple operand copy; trivial once place-projection works |
| `Repeat`             | Not Started | Array fill `[expr; N]` |
| `Ref`                | Not Started | Needs address-taken / alloca logic |
| `AddressOf`          | Not Started | Raw pointer to place |
| `Len`                | Not Started | Slice length extraction |
| `Cast`               | Not Started | `sext`/`zext`/`bitcast`/`ptr-to-int` etc. |
| `BinaryOp`           | Not Started | `add`/`sub`/`mul`/etc.; straightforward once Types mapped |
| `CheckedBinaryOp`    | Not Started | Returns `(result, overflow_flag)` tuple |
| `NullaryOp`          | Not Started | `SizeOf`/`AlignOf` — needs `rustc_target::abi` |
| `UnaryOp`            | Not Started | `neg`/`not` |
| `Discriminant`       | Not Started | Read enum tag |
| `Aggregate`          | Not Started | Struct/array/enum construction |
| `ShallowInitBox`     | Not Started | Box initialisation intrinsic |
| `CopyForDeref`       | Not Started | Implicit copy through deref coercion |

### MIR Statements

| Statement Kind       | Status      | Notes |
|----------------------|-------------|-------|
| `Assign`             | Not Started | Core statement; blocked on Rvalue and place-projection |
| `FakeRead`           | Not Started | Borrow-check artifact; no-op at codegen |
| `SetDiscriminant`    | Not Started | Write enum tag to place |
| `Deinit`             | Not Started | Poison / undef; can be no-op initially |
| `StorageLive`        | Not Started | Alloca lifetime hint; can be ignored initially |
| `StorageDead`        | Not Started | Alloca lifetime hint; can be ignored initially |
| `Retag`             | Not Started | Stacked-borrows retag; no-op without Miri |
| `AscribeUserType`    | Not Started | Type-ascription annotation; no-op at codegen |
| `Coverage`           | Not Started | LLVM coverage instrumentation hooks |
| `Intrinsic`          | Not Started | Non-diverging intrinsics (e.g. `assume`) |
| `ConstEvalCounter`   | Not Started | Interpreter step counter; no-op at codegen |
| `Nop`                | Not Started | Explicit no-op; trivial to handle |
