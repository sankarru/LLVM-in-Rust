# RFC Process

This document describes the lightweight RFC (Request for Comments) process used
to govern breaking changes to the LLVM-in-Rust public API and IR semantics.

---

## When an RFC is required

An RFC is required before merging any change that:

- **Alters a public type or function signature** — changing parameter types,
  return types, or generic bounds on anything `pub` in a released crate.
- **Removes a public type, trait, function, or module** — including renaming
  (rename = remove old + add new).
- **Changes IR semantics** — adding, removing, or redefining a variant of
  `InstrKind`, `TypeData`, or `ConstantData` in `llvm-ir`.
- **Breaks object-file format compatibility** — changes to LRIR magic bytes,
  version fields, record layout, or ELF/Mach-O section structure that would
  make objects produced by the old code unreadable by the new code (or vice
  versa).
- **Adds a mandatory method to a public trait** — e.g. adding a required method
  to `IselBackend`, `Emitter`, `FunctionPass`, or `ModulePass` that downstream
  implementors must provide.

If you are unsure, open an issue first and ask. Erring on the side of writing
an RFC is always acceptable.

---

## When an RFC is NOT required

No RFC is needed for:

- **Purely additive changes** — new optional methods (with default impls), new
  crates, new `pub` types that nothing existing depends on.
- **Bug fixes that restore documented behaviour** — the fix must not change any
  stable, intentionally-specified API surface; only bring behaviour in line with
  what the docs already promise.
- **Documentation-only changes** — changes to `.md` files, doc comments,
  examples, or tests with no production-code impact.
- **Performance improvements** with no API surface change — e.g. a faster
  register allocator that keeps all public types and traits identical.
- **Internal refactors** — renaming or restructuring `pub(crate)` items,
  private helpers, or anything not exposed in public API docs.

---

## RFC lifecycle

```
Draft  -->  Review (min 7 days)  -->  FCP (3 days)  -->  Accepted / Rejected
```

1. **Draft**: Author opens a PR to `docs/rfcs/` with label `rfc`.  The file is
   named `NNNN-short-title.md` where `NNNN` is the next available four-digit
   number (use `0000` until a maintainer assigns a number).
2. **Review**: At least 7 calendar days of open discussion.  Anyone may comment.
   The author may revise the RFC in response to feedback; significant revisions
   reset the 7-day clock.
3. **Final Comment Period (FCP)**: A maintainer posts an explicit `r+ fcp` comment
   to signal that the RFC is ready for a final decision.  FCP lasts 3 calendar
   days to allow any last objections.
4. **Accepted**: After FCP, a maintainer merges the RFC PR.  The RFC file is
   now the authoritative spec for the implementation PR(s).
5. **Rejected**: A maintainer closes the RFC PR with a brief explanation.
   Rejection is not permanent — revised RFCs may be resubmitted.

### After acceptance

- Implementation PRs reference the RFC: `Implements RFC NNNN`.
- The RFC file is updated in-place only to add an `## Implementation` section
  linking the implementation PRs.  The rest of the RFC is immutable once merged.

---

## How to submit an RFC

1. Fork the repository and create a branch named `rfc/short-description`.
2. Copy `docs/rfcs/0000-template.md` to `docs/rfcs/0000-short-title.md`.
3. Fill in every section of the template.  Leave the number as `0000` — a
   maintainer will assign the final number before merging.
4. Open a PR against `main` targeting only `docs/rfcs/`.
   > **Tip**: append `?template=rfc.md` to the PR creation URL so GitHub
   > pre-loads the RFC PR checklist from `.github/PULL_REQUEST_TEMPLATE/rfc.md`.
5. Add the `rfc` label to the PR.
6. Post a link to the PR in the relevant issue (if one exists).

---

## Who can approve

Any maintainer can approve an RFC by posting `r+ fcp` to start the Final
Comment Period, and can merge the RFC PR after FCP concludes.  Maintainers are
GitHub org owners of the `yudongusa/LLVM-in-Rust` repository.

Implementors are encouraged to volunteer to implement accepted RFCs by claiming
the tracking issue.
