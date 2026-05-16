# Contributing to LLVM-in-Rust

LLVM-in-Rust is a pure-Rust reimplementation of the LLVM compiler pipeline — no C++, no FFI. Contributions at every layer are welcome: IR analysis, optimization passes, target backends, the IR text-format parser, and tooling.

---

## 1. Getting Started

### Prerequisites

- **Rust stable toolchain** — install via [rustup](https://rustup.rs/).
- **`cargo`** — included with any standard Rust installation.
- **`llvm-as` / `llvm-dis`** (optional) — required only for the differential test suite in `src/llvm-ir-parser/tests/differential.rs`. Install LLVM via your system package manager (`brew install llvm`, `apt install llvm`, etc.) and make sure `llvm-as` is on your `PATH`.

### First-time setup

```sh
git clone https://github.com/your-org/llvm-in-rust.git
cd llvm-in-rust
cargo build              # Build all crates (debug)
cargo build --release    # Release build
cargo test               # Run the full test suite
cargo clippy             # Lint (fix all warnings before opening a PR)
cargo fmt                # Format (must be clean before opening a PR)
cargo check              # Fast type-check without codegen
```

### Running a subset of tests

```sh
# Test a single crate
cargo test -p llvm-transforms

# Test a single crate by test name substring
cargo test -p llvm-transforms mem2reg

# Test one specific test function
cargo test -p llvm-analysis dominators::tests::simple_diamond
```

### Running benchmarks

```sh
cargo bench -p llvm-in-rust-bench
```

Benchmark code lives in `src/llvm-bench/benches/pipeline.rs`. CI enforces a 5 % performance budget; see [§6 PR Process](#6-pr-process) for what to do if you exceed it.

---

## 2. Adding an Optimization Pass

Optimization passes live in `src/llvm-transforms/`. A pass either transforms a single function (`FunctionPass`) or the whole module (`ModulePass`).

### Step 1 — Create the pass file

```sh
touch src/llvm-transforms/src/my_pass.rs
```

### Step 2 — Implement the trait

The traits are defined in `src/llvm-transforms/src/pass.rs`:

```rust
/// A pass that transforms a single function.
pub trait FunctionPass {
    /// Apply this pass to `func`. Returns `true` if the IR was modified.
    fn run_on_function(&mut self, ctx: &mut Context, func: &mut Function) -> bool;

    /// Human-readable name used in diagnostics.
    fn name(&self) -> &'static str;
}

/// A pass that transforms an entire module.
pub trait ModulePass {
    /// Apply this pass to `module`. Returns `true` if the IR was modified.
    fn run_on_module(&mut self, ctx: &mut Context, module: &mut Module) -> bool;

    /// Human-readable name used in diagnostics.
    fn name(&self) -> &'static str;
}
```

A minimal `FunctionPass` skeleton:

```rust
// src/llvm-transforms/src/my_pass.rs
use crate::pass::FunctionPass;
use llvm_ir::{Context, Function};

pub struct MyPass;

impl FunctionPass for MyPass {
    fn name(&self) -> &'static str {
        "my-pass"
    }

    fn run_on_function(&mut self, _ctx: &mut Context, func: &mut Function) -> bool {
        // Inspect and/or mutate `func.instructions` / `func.blocks`.
        // Return true if you modified anything.
        false
    }
}
```

If your pass needs inter-procedural information (e.g., the call graph), implement `ModulePass` instead. Use `FunctionPassAdapter` to apply a `FunctionPass` to every function in a module — the `PassManager` does this automatically when you call `add_function_pass`.

### Step 3 — Export the pass

Add a module declaration and a re-export to `src/llvm-transforms/src/lib.rs`:

```rust
pub mod my_pass;

pub use my_pass::MyPass;
```

### Step 4 — Optionally add to the pipeline

To include your pass in the standard `-O2`/`-O3` pipeline, edit `build_pipeline()` in `src/llvm-transforms/src/pipeline.rs`:

```rust
OptLevel::O2 => {
    // ... existing passes ...
    pm.add_function_pass(MyPass);
    // ...
}
```

### Step 5 — Write unit tests

Use `llvm_ir_parser::parser::parse` to build a fixture module from a `.ll` snippet, then assert on the transformed IR:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use llvm_ir_parser::parser::parse;
    use crate::pass::PassManager;

    #[test]
    fn my_pass_does_something_useful() {
        let src = r#"
define i32 @f(i32 %x) {
entry:
  %r = add i32 %x, 0
  ret i32 %r
}
"#;
        let (mut ctx, mut module) = parse(src).expect("valid IR");
        let mut pm = PassManager::new();
        pm.add_function_pass(MyPass);
        let changed = pm.run(&mut ctx, &mut module);
        assert!(changed);
        // inspect module.functions[0] ...
    }
}
```

Integration tests go in `src/llvm-transforms/tests/`.

---

## 3. Adding a Target Instruction (x86 example)

The x86 backend is in `src/llvm-target-x86/src/`. The same pattern applies to the AArch64 backend (`src/llvm-target-arm/`) and RISC-V backend (`src/llvm-target-riscv/`).

### Step 1 — Add an `MOpcode` constant

Opcodes are plain `u32` wrappers defined in `src/llvm-target-x86/src/instructions.rs`. Pick an unused hex value in the appropriate group:

```rust
// src/llvm-target-x86/src/instructions.rs
use llvm_codegen::isel::MOpcode;

// Existing opcodes as reference:
pub const MOV_RR: MOpcode  = MOpcode(0x00);  // mov dst, src  (reg→reg)
pub const ADD_RR: MOpcode  = MOpcode(0x10);  // add dst, src
pub const AND_RR: MOpcode  = MOpcode(0x20);  // and dst, src

// New opcode — pick a slot that doesn't collide with existing ones:
pub const BSWAP_R: MOpcode = MOpcode(0x1A);  // bswap dst  (byte-swap 64-bit register)
```

Opcodes are grouped by category (data movement `0x0x`, arithmetic `0x1x`, bitwise `0x2x`, shifts `0x3x`, comparisons `0x4x`, control flow `0x5x`, stack `0x6x`, misc `0x7x`, SIMD `0x8x`). Stay within the right group.

### Step 2 — Emit the instruction in the lowering pass

Handle the new opcode in `src/llvm-target-x86/src/lower.rs`. Find the `lower_instr` method on `X86Backend` and add a match arm for the IR instruction you want to lower to `BSWAP_R`:

```rust
// src/llvm-target-x86/src/lower.rs  (inside lower_instr)
InstrKind::Bswap { val } => {
    let src = self.value_to_vreg(val, mf, func, ctx);
    let dst = mf.new_vreg();
    mf.push(MInstr::new(MOV_RR).with_dst(dst).with_vreg(src));
    mf.push(MInstr::new(BSWAP_R).with_dst(dst));
    self.set_vreg_for(iid, dst);
}
```

### Step 3 — Add encoding in the emitter

Open `src/llvm-target-x86/src/encode.rs` and add a match arm in `encode_instr`:

```rust
// src/llvm-target-x86/src/encode.rs  (inside encode_instr)
BSWAP_R => {
    // REX.W + 0F C8+rd  (BSWAP r64)
    if let Some(dst) = instr.dst {
        let r = PReg(dst.0 as u8);
        maybe_rex(ctx, true, PReg(0), r); // REX.W, no extra bits
        ctx.emit(0x0F);
        ctx.emit(0xC8 | (reg_enc(r) & 7));
    } else {
        ctx.emit(0x90); // fallback NOP
    }
}
```

### Step 4 — Write a test verifying emitted bytes

```rust
// src/llvm-target-x86/tests/encode_bswap.rs  (or inline in encode.rs)
#[test]
fn bswap_rax_encodes_correctly() {
    use llvm_codegen::isel::{MInstr, MachineFunction, PReg};
    use crate::encode::X86Emitter;
    use crate::instructions::BSWAP_R;
    use llvm_codegen::emit::{Emitter, ObjectFormat};

    let mut mf = MachineFunction::new("f");
    let b = mf.new_block("entry");
    mf.set_entry(b);
    mf.push_to(b, MInstr::new(BSWAP_R).with_dst_preg(PReg(0 /* RAX */)));

    let mut emitter = X86Emitter::new(ObjectFormat::Elf);
    let section = emitter.emit_function(&mf);
    // REX.W (0x48) + 0F + C8 = bswap rax
    assert_eq!(&section.data[..3], &[0x48, 0x0F, 0xC8]);
}
```

---

## 4. Extending the IR Parser

The parser is a hand-rolled recursive-descent parser with 1-token lookahead.

- **Lexer**: `src/llvm-ir-parser/src/lexer.rs` — `Keyword` enum, `Token` enum, and `Lexer` struct.
- **Parser**: `src/llvm-ir-parser/src/parser.rs` — `Parser` struct with `parse_instr_kind` as the primary dispatch point for instruction parsing.

### Adding a new keyword

1. Add a variant to the `Keyword` enum in `lexer.rs`:

```rust
pub enum Keyword {
    // ... existing variants ...
    /// `freeze` keyword (freeze instruction).
    Freeze,
}
```

2. Map the source string to the variant in the `lex_keyword` function (or the `match` arm on the raw identifier string) inside `Lexer::next`:

```rust
"freeze" => Token::Kw(Keyword::Freeze),
```

### Adding a parse arm for a new instruction

In `parser.rs`, find `parse_instr_kind` and add a match arm. The keyword `Freeze` would look like:

```rust
// src/llvm-ir-parser/src/parser.rs  (inside parse_instr_kind)
Token::Kw(Keyword::Freeze) => {
    self.lex.next()?; // consume 'freeze'
    let val = self.parse_typed_value()?;
    Ok(InstrKind::Freeze { val })
}
```

Important: each arm must consume exactly the tokens it needs. The `peek()` borrow pattern used throughout the parser means you should avoid calling `self.err()` while holding a `&self.lex` reference; call `self.lex.next()?` first, then handle the error.

### Differential tests

If you add an instruction that LLVM also understands, add a round-trip test to `src/llvm-ir-parser/tests/differential.rs`. The test calls `llvm-as` as a golden oracle and compares the parsed output byte-for-byte:

```rust
#[test]
fn freeze_roundtrip() {
    let src = r#"
define i32 @f(i32 %x) {
entry:
  %y = freeze i32 %x
  ret i32 %y
}
"#;
    assert_roundtrip(src);  // helper in differential.rs
}
```

Differential tests are skipped automatically when `llvm-as` is not on `PATH`.

---

## 5. Writing Tests

| Kind | Where | How |
|---|---|---|
| Unit | `#[cfg(test)]` module inside the source file | Inline, no fixtures |
| Integration | `src/<crate>/tests/*.rs` | Separate files, can use fixtures |
| Differential | `src/llvm-ir-parser/tests/differential.rs` | Requires `llvm-as` on PATH |
| Performance | `src/llvm-bench/benches/pipeline.rs` | Criterion, CI-enforced budget |

### Unit tests

Put them directly in the `.rs` file they exercise. Use the `parse` entry point to build non-trivial IR fixtures rather than constructing the IR builder API by hand — it is far more readable:

```rust
#[cfg(test)]
mod tests {
    use llvm_ir_parser::parser::parse;

    #[test]
    fn constant_folding_add_zero() {
        let (mut ctx, mut module) = parse(r#"
define i32 @f(i32 %x) {
entry:
  %r = add i32 %x, 0
  ret i32 %r
}
"#).unwrap();
        // ... run pass, assert ...
    }
}
```

### Integration tests

Files in `src/<crate>/tests/` are compiled as separate test binaries. Integration tests may import helper modules from `src/<crate>/src/` using `use <crate>::...`.

### Performance tests

Add to `src/llvm-bench/benches/pipeline.rs`. Use `criterion::black_box` to prevent the optimizer from eliding your measured work. CI rejects regressions larger than 5 % unless you label the PR `perf-regression-accepted` with an explanation.

---

## 6. PR Process

1. **One issue per PR.** Reference the issue in the PR title: `closes #N`.
2. **All tests must pass**: `cargo test`
3. **Clippy clean**: `cargo clippy -- -D warnings`
4. **Formatted**: `cargo fmt --check`
5. **Breaking API changes** require an RFC filed in `docs/rfcs/` (create the directory and a numbered `.md` if it does not exist yet). Discuss on the issue before implementing.
6. **Performance regressions > 5 %** require either a fix or the `perf-regression-accepted` label with a written justification in the PR body.

### Checklist before opening a PR

```sh
cargo test                        # all tests green
cargo clippy -- -D warnings       # zero warnings
cargo fmt --check                 # no formatting drift
cargo build --release             # release build succeeds
cargo bench -p llvm-in-rust-bench # optional: verify no perf regression
```

---

## 7. Code Style

These rules override default Rust community conventions where they conflict.

- **No comments unless the WHY is non-obvious.** What the code does is apparent from reading it; explain why it does that when it would otherwise be surprising.
- **No multi-paragraph doc comments on internal helpers.** A single sentence is fine. Save prose for public API items that appear in `cargo doc` output.
- **Prefer editing an existing file to creating a new one.** New files require updating `lib.rs`, `Cargo.toml`, and any re-export lists. Do not add a new file unless the abstraction genuinely warrants it.
- **Arena allocation for IR nodes.** Never use `Box<Node>` in hot paths. IR types and constants are interned in `Context`; instructions and blocks live in pool `Vec`s on `Function`. If you add a new IR construct, allocate it in the appropriate pool.
- **SSA form throughout.** The IR must remain in SSA form after every pass. If your pass introduces non-SSA patterns (e.g., it removes a phi), clean them up before returning.
- **Avoid `unwrap()` in library code.** Propagate errors with `?` or return `Option`/`Result`. `unwrap()` is acceptable in tests and in clearly unreachable paths guarded by prior invariant checks.
- **No `#[allow(dead_code)]` on new items.** If a new item is unused, either use it or do not add it yet.
- **Keep match arms exhaustive.** Do not use a catch-all `_ => unreachable!()` arm in `encode_instr` or `lower_instr` — add a `NOP` fallback only when intentionally punting on an opcode, and leave a `// TODO:` comment explaining what should go there.

---

## Crate Dependency Map

```
llvm-ir                (no deps — foundation)
  ├── llvm-ir-parser   (depends: llvm-ir)
  ├── llvm-analysis    (depends: llvm-ir)
  ├── llvm-transforms  (depends: llvm-ir, llvm-analysis)
  ├── llvm-codegen     (depends: llvm-ir, llvm-analysis)
  │     ├── llvm-target-x86   (depends: llvm-ir, llvm-codegen)
  │     ├── llvm-target-arm   (depends: llvm-ir, llvm-codegen)
  │     └── llvm-target-riscv (depends: llvm-ir, llvm-codegen)
  ├── llvm-bitcode     (depends: llvm-ir)
  └── llvm             (top-level re-exports — depends: all above)
```

Keep this hierarchy strict: lower crates must not depend on higher ones. If you need to share a type between two crates at the same level, move it down to their common ancestor.
