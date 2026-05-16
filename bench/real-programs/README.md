# Real-Program Compilation Benchmarks

Measures the runtime quality of our codegen against `clang -O2` on three
scalar-computation benchmarks.

## How to run

```bash
# From the workspace root
bash bench/real-programs/run.sh
```

Prerequisites: Rust toolchain (`cargo`), `clang`, `cc` (system linker).

## Pipeline

```
fixtures/bench_X.ll ──► llvm-ir-compile ──► bench_X.o ──► cc ──► bench_X_ours
fixtures/bench_X.c  ──► clang -O2       ──────────────────────► bench_X_ref
```

The `.ll` fixtures are hand-crafted LLVM IR (same style as the smoke tests)
that our full pipeline — `parse → mem2reg → lower → regalloc → spill-reload →
emit → link` — can compile correctly.  The `.c` files are equivalent C sources
compiled with `clang -O2` as the performance reference.

## Benchmarks

| Program   | What it computes                              | Expected exit code |
|-----------|----------------------------------------------|--------------------|
| `fib`     | `fib(30) × 20M iterations` (iterative)      | 40                 |
| `gcd`     | `GCD(100003+i, 99991)` × 50M iterations      | 1                  |
| `collatz` | Collatz steps for all `n` in 1..500 000      | 51                 |

All results are returned as `result % 100` to fit in an 8-bit exit code that
both the reference and our binary agree on (correctness gate).

## Results (macOS arm64, Apple M-series, 2026-05)

| program  | clang -O2 | ours    | slowdown | correctness  |
|----------|-----------|---------|----------|--------------|
| fib      | 0.008 s   | 0.480 s | **60×**  | OK (exit=40) |
| gcd      | 1.364 s   | 1.826 s | **1.3×** | OK (exit=1)  |
| collatz  | 0.051 s   | 0.214 s | **4.2×** | OK (exit=51) |

Binary sizes are identical (16 848 bytes) because both pipelines produce a
minimal Mach-O object that links against the same system libraries.

### Notes on the fib result

Even with `volatile long iters`, clang -O2 applies loop-strength-reduction and
partially-unrolls the inner Fibonacci loop into a straight-line sequence.  Our
pipeline emits one load/add/store per iteration without any such optimisation,
hence the 60× gap.  This measures missing loop optimisations, not a fundamental
codegen deficiency.

## Known pipeline limitations

These benchmarks are designed to work within the current pipeline's capabilities:

- **Scalar variables only** — all allocas are promoted to SSA by `mem2reg`;
  array/struct accesses (GEP + load/store chains) are not yet emitted correctly.
- **No external function calls** — `printf`, `malloc`, etc. require relocations
  that the current `BLR`-only call-lowering does not support.
- **No floating point** — FP arithmetic stubs emit `MOV_IMM 0` placeholders.
- **Mach-O object files lack LC_BUILD_VERSION** — the system linker emits a
  harmless platform-load-command warning.

## Top 3 optimisation gaps

1. **Loop optimisations (fib, 60×)** — LICM moves loop-invariant constant
   materialisations out of inner loops; strength reduction replaces multiply-by-2
   with shifts; loop unrolling amortises branch/phi overhead.  Implementing even
   a basic LICM pass would close most of this gap.

2. **Instruction selection for SREM (collatz, 4.2×)** — we lower `srem i64 x, 2`
   as `SDIV + MUL + SUB` (3 instructions).  AArch64 provides `MSUB`
   (`a - b*c` in one cycle) and strength-reduces `%2` to an AND mask.  clang
   emits `AND x, x, #1` for the parity check instead of a full division.

3. **Register allocation quality (all)** — our linear-scan allocator does not
   split live ranges or use caller-saved registers aggressively.  Live ranges for
   loop-carried phi values span the entire loop body, preventing the allocator
   from reusing registers for short-lived temporaries inside the loop.

## Adding new benchmarks

1. Write a `fixtures/bench_NAME.ll` using `alloca`/`load`/`store` for scalar
   variables (mem2reg will promote them) with no external function calls.
2. Write a matching `fixtures/bench_NAME.c` (used only for the reference binary).
3. Add `NAME` to the benchmark list in `run.sh`.
