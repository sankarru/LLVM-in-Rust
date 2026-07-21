---
name: milestone-z-rc-burnin
description: Execute Milestone Z release-candidate burn-in work for issue #385/#93: unblock parser/fuzz findings, collect exact-commit evidence, publish the go/no-go bundle, and update roadmap status.
---

# Milestone Z RC Burn-In

Use this skill for the remaining production-readiness roadmap work in
Milestone Z (#385) and parent roadmap #93.

## When to Use

- A fuzz, differential, sanitizer, platform, release-artifact, or performance
  gate blocks Milestone Z.
- An RC evidence PR, go/no-go bundle, or #93/#385 status update is needed.
- A stalled Z PR needs review/merge/rerun coordination between agents.

Do not use this skill for unrelated feature work or broad refactors.

## Current Gate Order

1. Clear any release-blocking finding with a focused issue and PR.
2. Require an explicit review comment from another agent or maintainer before
   merge; do not self-approve.
3. Merge the blocker fix after all PR checks are green.
4. Dispatch the exact main-commit Z evidence reruns:
   - `Fuzzing (LLVM-Stress + CSmith)` with the required CSmith count.
   - `fuzz-differential`.
   - `Sanitizer and UB hardening`.
   - Any exact-commit performance, platform, differential, release-artifact,
     golden, compatibility, and docs gates required by the RC checklist.
5. If any rerun fails, download logs/artifacts, create one focused issue, fix
   it in one PR, and restart this gate order.
6. Only after all evidence is green, merge the RC evidence tooling/status PRs
   and publish the #385/#93 go/no-go comment.

## Evidence Handling

- Record run IDs, commit SHAs, artifact IDs, and local artifact paths in the
  agent memory before pausing or waiting for long CI jobs.
- Use reminders for CI/review waits instead of leaving a terminal sleep alive.
- Preserve raw fuzz artifacts before reducing them; reduced inputs are useful
  only after the raw reproducer is safely stored.
- Distinguish scheduled burn-in evidence from exact-candidate evidence. A
  scheduled green run is supportive, but an RC decision must cite the intended
  pinned commit.

## Validation Before Merge

For docs-only Z tooling changes:

```bash
bash -n scripts/rc_evidence_bundle.sh
bash -n scripts/release_candidate_protocol.sh
scripts/release_candidate_protocol.sh
git diff --check
```

For parser/fuzz fixes, at minimum run:

```bash
cargo test -p llvm-in-rust-ir-parser --test negative_inputs
cargo fmt --check -p llvm-in-rust-ir-parser
cargo clippy -p llvm-in-rust-ir-parser -- -D warnings
cargo +nightly fuzz run parser <raw-or-minimized-reproducer> -- -runs=1
```

If the fix touches shared crates, broaden to the relevant crate tests and
workspace tests before review.

## Coordination Rules

- Use the active Slock task thread for status, ownership, and handoffs.
- Split work explicitly:
  - one agent owns implementation,
  - the other owns review or independent evidence verification.
- Keep #398-style evidence PRs held until all blocker fixes are merged and the
  exact main-commit reruns are green.
- After a merge, close the associated issue and update #385/#93 in the same
  work session.
