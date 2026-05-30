# Error-Handling Audit — Production Panic Sites

Issue #383 (Milestone X) item 1: audit every non-test `panic!` / `unwrap()` /
`expect()` call site in the runtime-facing crates and classify each as
**test-only**, **invariant-checked**, or **production-facing**.

Source-of-truth scan: `scripts/audit_error_handling.py` (added in this PR).

## Methodology

The script walks every `.rs` file under `src/`, skipping `tests/`,
`examples/`, and `benches/` directories, and skipping every region inside
`#[cfg(test)] mod tests { … }` (tracked by `{` / `}` brace counting after the
`#[cfg(test)]` attribute).  Inside the remaining "production" lines it counts
the three real panicking forms:

| Pattern | Regex | Why this excludes false positives |
|---|---|---|
| `panic!` macro | `(?<![A-Za-z_])panic!\s*\(` | Word-boundary lookbehind avoids `expand_panic!`, `panicking::panic!`, etc. |
| `.unwrap()` on Option/Result | `\.unwrap\s*\(\s*\)` | The empty parens distinguish std `unwrap` from `unwrap_or…`. |
| `.expect("msg")` on Option/Result | `\.expect\s*\(\s*"` | The leading string literal distinguishes std `expect` from the parser's lexer-style `lex.expect(&Token::…)` method. |

## Crate-level summary (2026-05-30)

| Crate | Production panic sites | Tests-only |
|---|---:|---:|
| `llvm-ir` | 76 | 0 |
| `llvm-transforms` | 14 | 75 |
| `llvm` | 7 | 7 |
| `llvm-target-wasm` | 5 | 11 |
| `llvm-codegen` | 4 | 22 |
| `llvm-rustc-backend` | 3 | 26 |
| `llvm-ir-parser` | 2 | 15 |
| `llvm-bitcode` | 1 | 37 |
| `llvm-target-riscv` | 1 | 17 |
| `llvm-analysis` | 0 | 4 |
| `llvm-bench` | 0 | 1 |
| `llvm-jit` | 0 | 11 |
| `llvm-target-arm` | 0 | 0 |
| `llvm-target-x86` | 0 | 7 |
| **Total** | **113** | **233** |

## Classification of the 113 production sites

### A. Infallible writes (~70 sites — *not* user-reachable panics)

Bulk of `llvm-ir/src/printer.rs` writes are
`write!(out: &mut String, …).unwrap()`.  Writing to a `String` is infallible
under `std::fmt::Write` — the underlying buffer cannot return an error.
This is the standard Rust idiom for printer code.  These are not real panic
sites: the `.unwrap()` is a static guarantee, not a runtime failure mode.

**Action:** no change required.  Optional follow-up could use the
`expect("write to String is infallible")` form to communicate intent, but
this is purely cosmetic.

### B. Internal-API invariants (~30 sites)

Examples:

- `let fid = self.current_function.expect("no current function");`
  — `llvm-ir/src/builder.rs`: builder methods called outside a positioned
  function.  This is API misuse by Rust callers, not user input.
- `sections.iter().position(|s| s.name == rodata_name).unwrap()`
  — `llvm-codegen/src/emit.rs`: the section is created earlier in the same
  function; the lookup cannot fail.
- `panic!("max 2 successors")` in `cfg.rs` / `loops.rs` — match arms
  documenting an invariant.  Reached only if the IR is internally
  inconsistent.
- `panic!("expected reloaded VReg operand")` in `regalloc.rs` — pre-condition
  on the spill-reload helper.

**Action:** no Result conversion needed; these are not reachable from
adversarial input.  We may tighten the messages over time, but they
correctly fail fast on a logic bug rather than silently producing wrong
output.

### C. Lexer post-peek invariants (2 sites in `llvm-ir-parser/src/lexer.rs`)

- `self.peeked.as_ref().unwrap()` — guaranteed `Some` because the surrounding
  code only enters this arm after `self.peek_some()` returned `true`.
- `self.advance().unwrap()` — same pattern.

Both are internal lookahead invariants, not user-input-driven panics on
malformed IR.  The user-input-driven errors in the parser already return
`Result<…, ParseError>`.

**Action:** consider rewriting as `if let Some(t) = …` for cleanliness, but
no Result API change.

### D. Doc-comment examples (2 sites)

`src/llvm-bitcode/src/lib.rs:20` and `src/llvm-codegen/src/thinlto.rs:24` —
matches inside `//! …` doc comments showing example usage with `.unwrap()`
for brevity.  Not real call sites.

**Action:** none.

### E. CLI argument parsing (~7 sites in `llvm-test-suite-compat.rs`)

`args.next().expect("--suite-dir value")` and similar.  These are operator-
facing CLI binaries used in CI; the `expect` message is the diagnostic.  Not
a security boundary.

**Action:** could be improved with proper `eprintln!` + non-zero exit, but
this is QoL, not a hardening blocker.

## Key finding

**The external-input-facing surfaces are already structured-error clean:**

| Surface | External input source | Error type today |
|---|---|---|
| `llvm-ir-parser::parser::parse(&str)` | Untrusted `.ll` text | `Result<_, ParseError>` |
| `llvm-bitcode::read_bitcode(&[u8])` | Untrusted LRIR bytes | `Result<_, BitcodeError>` |
| `llvm-bitcode::read_llvm_bc(&[u8])` | Untrusted LLVM `.bc` | `Result<_, BitcodeError>` |
| `llvm-jit::SimpleJit::add_module(…)` | Trusted-IR `(Context, Module)` | `Result<_, JitError>` |

The audit found **zero production panic sites that are reachable from a
malformed external input** in the parser, bitcode readers, or JIT entry
point.  Every panic in these paths is either a tests-only assertion, a
post-peek/post-allocation lookahead invariant, or an API-misuse expect on a
Rust caller's positioning.

## Implications for the Milestone X follow-ups

Because the structured-error story is already in place at the entry-point
boundary, the remaining Milestone X work shifts from *"add structured error
types"* to *"bound the resources those error paths can consume before
failing"* and *"document the safety contract."*

| Item | Original scope | Refined scope |
|---|---|---|
| Replace production-facing panics with structured errors | Convert remaining sites | No-op — already structured. Optional: classify each retained panic as invariant-only (`debug_assert!`-style) for future polish. |
| Resource limits in parser/optimizer | New `ParseLimits` / `OptLimits` structs | Add with documented defaults; thread through `parse` / pass manager. |
| CLI flags for limits | Surface on the CLI binaries | Add `--max-fn` / `--max-instr` etc. on `llvm-compile` / `llvm-ir-min` / `llvm-test-suite-compat`. |
| Negative/fuzz tests | Oversized IR, deep nesting, malformed EH | Mostly new — add `tests/` cases that trigger each `ParseError`/`BitcodeError` variant and each new resource-limit error. |
| Sandbox guidance doc | Non-sandbox security model | New doc page in `docs/`. |

This audit will be referenced by subsequent Milestone X PRs as the
classification of record.

## Reproducing the audit

```bash
python3 scripts/audit_error_handling.py src --json
```

Writes `/tmp/audit_prod_sites.json` with every (file, line, kind) classified
production site.

Refs #93, refs #383.
