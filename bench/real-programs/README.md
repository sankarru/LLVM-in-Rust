# Real-Program Compilation Benchmark

This directory contains a harness for measuring the end-to-end compilation
speed and code quality of the LLVM-in-Rust pipeline against Clang -O2 as a
reference.

## Purpose and Methodology

The harness compiles three scalar-only C benchmark programs through two paths:

1. **Reference path** — `clang -O2` (highly optimised, industry baseline)
2. **Ours path** — `clang -O0 -emit-llvm` (unoptimised IR) → `llvm-ir-compile`
   (our Rust-native pipeline: mem2reg + linear-scan regalloc) → native linker

This isolates the quality of our code generator and register allocator from
front-end optimisations: both paths start from the same C source, but our
pipeline sees unoptimised IR while Clang -O2 applies the full optimisation
stack.

The programs are deliberately scalar-only (no arrays, no heap allocations, no
external function calls) so that mem2reg promotes every `alloca`/`load`/`store`
triple to pure SSA and our pipeline can compile them without stubs.

## Prerequisites

- Rust toolchain with `cargo` in `PATH`
- `clang` (any recent version) in `PATH`
- `cc` (system linker, typically `gcc` or `clang`) in `PATH`
- A POSIX shell with `bash` ≥ 4

## How to Run

From the workspace root (the directory containing `Cargo.toml`):

```bash
bash bench/real-programs/run.sh
```

The script will:
1. Build `llvm-ir-compile` in release mode.
2. For each benchmark: compile with clang -O2, emit LLVM IR with clang -O0,
   compile IR with our pipeline, link, and run.
3. Time each binary (median of 3 runs).
4. Check correctness by comparing exit codes.
5. Print a formatted results table.

Intermediate files land in `bench/real-programs/.tmp/` and are left for
inspection.

## Benchmark Descriptions

### `bench_fib` — Iterative Fibonacci

Computes fib(30) = 832 040 iteratively, repeated 20 million times.
Returns `fib(30) % 100 = 40` as the exit code.

Exercises: inner/outer loop with three live variables (a, b, c), integer add.

### `bench_gcd` — Euclidean GCD

Computes the GCD of `100003 + (iter & 0xFFFF)` and `99991` for 50 million
iterations.  Returns `result % 100` as the exit code.

Exercises: while-loop with remainder (`%`) inside an outer for-loop, integer
division, multiple live variables.

### `bench_collatz` — Collatz Sequence

For each starting value 1..500 000, counts the number of steps until the
Collatz sequence reaches 1.  Returns `steps % 100` for the last starting value.

Exercises: nested while-loops, conditional branch (even/odd split), mixed
multiply and divide, long loop trip count.

## Known Limitations

The current pipeline does not support:

- **Floating-point** — no FP registers or instructions are allocated.
- **Arrays / pointers after mem2reg** — only heap-free, scalar programs can be
  fully compiled.  Any remaining `alloca` after mem2reg produces a stub `mov 0`
  and will silently compute the wrong answer.
- **External function calls** — `printf`, `malloc`, etc. are not yet lowered.
- **Variadic functions** — ABI handling for variadic calls is incomplete.
- **Optimisation** — only mem2reg runs; no constant propagation, loop
  optimisation, or instruction scheduling beyond what mem2reg enables.

## Placeholder Results Table

| program  | clang-O2 (s) | ours (s) | slowdown | clang size (B) | ours size (B) |
|----------|-------------|----------|----------|----------------|---------------|
| fib      |  (TBD)      |  (TBD)   |  (TBD)x  |  (TBD)         |  (TBD)        |
| gcd      |  (TBD)      |  (TBD)   |  (TBD)x  |  (TBD)         |  (TBD)        |
| collatz  |  (TBD)      |  (TBD)   |  (TBD)x  |  (TBD)         |  (TBD)        |

Run `bash bench/real-programs/run.sh` to fill in the table for your machine.

## Expected Performance Gap and Optimisation Opportunities

We expect a **5–30× slowdown** compared to clang -O2 for these benchmarks.
The main contributors are:

1. **No loop optimisation** — clang -O2 auto-vectorises and applies
   loop-invariant code motion; we do neither.
2. **Linear-scan register allocator** — produces more spill/reload code than an
   optimal graph-colouring allocator, especially for inner loops with many live
   variables.
3. **No instruction scheduling** — instructions are emitted in IR order; a
   scheduler could hide latency by reordering independent instructions.
4. **No constant folding after lowering** — immediate-materialisation sequences
   (MOVZ/MOVK on AArch64) are not folded with subsequent arithmetic.
5. **No peephole optimisation** — redundant move sequences generated during
   phi-destruction are not eliminated.

The top three improvements that would shrink the gap most:
- Run constant propagation + DCE before codegen (reduces live range pressure).
- Implement a simple register coalescing pass to eliminate copy chains from
  phi-destruction.
- Add basic loop-invariant code motion (LICM) to hoist loop-constant
  calculations out of inner loops.
