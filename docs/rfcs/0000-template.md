# RFC NNNN: [Title]

- **Start date**: YYYY-MM-DD
- **RFC PR**: (leave blank until you open the PR)
- **Tracking issue**: (link to the GitHub issue this addresses, if any)

---

## Summary

One paragraph explaining the change.  What is being changed and what problem
does it solve?

---

## Motivation

Why does this change need to happen now?  What pain point, correctness issue,
or design limitation makes the status quo unacceptable?  Include concrete
examples where possible (e.g. code that is currently broken or confusing).

---

## Guide-level explanation

Explain the change as you would to a new contributor.  What does the API look
like after this RFC?  Walk through the common use case with a short code snippet
or before/after example.  Skip internal implementation details here — focus on
what users of the crate will see and do differently.

---

## Reference-level explanation

Precise description of every change being made:

### API changes (before / after)

```rust
// Before
pub fn example(x: OldType) -> OldReturn;

// After
pub fn example(x: NewType) -> NewReturn;
```

List every affected type, function, trait method, or module.

### IR semantics changes (if applicable)

Describe any changes to `InstrKind`, `TypeData`, `ConstantData`, or related
enums, including new variants, renamed variants, and removed variants.

### Format compatibility (if applicable)

Describe any changes to the LRIR binary format or ELF/Mach-O section structure.
State whether old objects remain readable after the change and how to migrate.

### Migration guide

What do downstream users need to change in their code?  Provide a step-by-step
migration path with code examples.

---

## Drawbacks

What are the costs of this change?  Consider:
- Churn for downstream users
- Increased complexity in the implementation
- Performance implications
- Anything that might make this hard to revert later

---

## Alternatives considered

What other designs were considered and why were they rejected?  Even if the
alternative is "do nothing", explain why the status quo is worse than the
proposed change.

---

## Unresolved questions

List any open questions that must be answered before implementation begins, or
that can be deferred to a follow-up RFC.  For example:
- Are there edge cases in the migration path that need more thought?
- Should a related but separable concern be addressed here or in a follow-up?
