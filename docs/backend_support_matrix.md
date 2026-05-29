# Backend Support Matrix

Issue #384 tracks the production-readiness requirement that backend maturity
claims are explicit, test-backed, and machine-checkable.

The CI-readable source of truth is
[`docs/backend_support_matrix.json`](backend_support_matrix.json). This Markdown
file explains how to read and maintain that fixture.

## Status Values

| Status | Meaning | Required fixture data |
|---|---|---|
| `supported` | The capability is in the scoped pilot support contract for that backend. | At least one evidence command and path. |
| `partial` | A tested subset exists, but limitations remain. | Evidence plus a `limitations` field. |
| `experimental` | Useful for tests or pilots, but not production-supported. | A marker such as `experimental:<area>` plus limitations. |
| `unsupported` | Not part of the production contract. | A marker such as `not_supported:<area>` plus limitations. |

The validator rejects missing cells, unsupported cells without explicit markers,
and supported/partial cells without evidence.

## Backend Matrix Summary

| Target | ABI | Calls | Varargs | Aggregates | Atomics | FP/SIMD | EH/unwind | Debug info | Object format | Relocations | Link/run |
|---|---|---|---|---|---|---|---|---|---|---|---|
| x86-64 | supported | supported | partial | partial | supported | supported | partial | supported | supported | partial | supported |
| AArch64 | partial | supported | unsupported | partial | supported | partial | partial | supported | partial | partial | partial |
| RISC-V RV64GC | partial | partial | unsupported | partial | supported | partial | experimental | partial | supported | partial | experimental |
| WebAssembly | partial | partial | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported | supported | unsupported | experimental |

## Release-Blocking End-to-End Evidence

The fixture also names the workflows and commands that make end-to-end coverage
visible in CI:

| Area | Workflow | Scope |
|---|---|---|
| C frontend | `.github/workflows/c-frontend-integration.yml` | clang -> LLVM IR -> LLVM-in-Rust codegen -> execute fixtures. |
| rustc backend smoke | `.github/workflows/backend-support-contract.yml` | Stable rustc-backend shim tests are release-blocking; nightly `rustc_private` wiring stays experimental. |
| LTO | `.github/workflows/backend-support-contract.yml` | Top-level LTO facade and ThinLTO object payload tests. |
| Debug/unwind | `.github/workflows/interoperability-conformance.yml` | DWARF/CodeView and unwind-object metadata checks. |
| Sanitizer-instrumented output | `.github/workflows/sanitizers.yml` | PR-blocking ASan/Miri lanes and scheduled/manual TSan lane. |

## Known Scope Decisions

- Wasm TODOs are explicitly scoped as unsupported or experimental in the JSON
  fixture: arbitrary CFG/relooper completeness, stack-frame allocation for
  `alloca`, loop phi destruction, indirect calls, external imports, FP/SIMD, and
  atomics are not production-supported yet.
- rustc backend TODOs are scoped as experimental. The stable shim smoke is
  release-blocking, but real `rustc_private`/`rustc_codegen_ssa` nightly-driver
  wiring is not a stable production contract.
- CFI/unwind and relocation gaps must either appear as `partial`/`experimental`
  cells with limitations or be promoted to known issues before a release
  manager can sign off the corresponding backend.

## Maintenance Rules

Run the validator after changing any backend support claim:

```bash
scripts/backend_support_contract.sh validate
```

When promoting a cell from `partial`, `experimental`, or `unsupported` to
`supported`, add a concrete evidence command and path in the JSON fixture in the
same PR. Do not update README or roadmap status before the corresponding support
contract check is green.
