# Security Policy

## Supported Versions

LLVM-in-Rust is currently in the `0.x` pre-1.0 phase. Security fixes are applied
as follows:

| Version | Supported |
|---------|-----------|
| `main` branch (latest) | Yes — always patched |
| Latest published `0.x.y` patch | Yes |
| Older `0.x` minors | Critical fixes only, for 90 days after the next minor release |
| Versions before the current minor | No |

Once the project reaches `1.0.0`, standard SemVer patch releases will carry all
security fixes for the current minor series.

---

## Reporting a Vulnerability

**Please do not open a public GitHub issue for security vulnerabilities.**

The preferred reporting channel is **GitHub Private Security Advisories**:

1. Go to the repository on GitHub.
2. Click **Security** → **Advisories** → **Report a vulnerability**.
3. Fill in the advisory form and submit.

This keeps the details confidential until a patch is ready and gives you direct
communication with the maintainers without public exposure.

### What to include in your report

- A clear description of the vulnerability and the affected component(s).
- The affected versions (or commit range, if known).
- Step-by-step reproduction instructions, ideally a minimal `.ll` file or Rust
  snippet that triggers the issue.
- The potential impact: what an attacker could achieve by exploiting this.
- Any suggested mitigations or fixes, if you have them.

### Email (secondary channel)

If you prefer not to use the GitHub advisory workflow, you may send a report to
`security@llvm-in-rust.dev`. Encrypt your message with the maintainer GPG key
if the content is sensitive (key available on the GitHub profile). The GitHub
advisory workflow is preferred because it integrates with the patch and release
process automatically.

---

## Response SLO

| Milestone | Target |
|-----------|--------|
| Acknowledge receipt of report | Within **7 days** |
| Initial assessment (confirmed / not a security issue) | Within **14 days** |
| Patch available for critical-severity issues | Within **30 days** |
| Patch available for high-severity issues | Within **60 days** |

For lower-severity issues (medium / low / informational), fixes are scheduled
with the normal release cadence. We will keep you informed of progress.

### Credit

Reporters who follow responsible disclosure will be credited by name (or handle,
if preferred) in the release notes and the `CHANGELOG.md` entry for the fixing
release. Please let us know your preferred credit attribution when you report.

---

## Scope

### In scope

- **Memory safety** — issues in the IR, parser, optimizer, or codegen pipeline
  that could cause memory unsafety in downstream callers.
- **Incorrect code generation** — compiler bugs that silently produce machine
  code with incorrect or unsafe semantics (wrong output, uninitialized reads,
  undefined behaviour in the generated binary).
- **Denial of service via crafted input** — maliciously crafted `.ll` files that
  cause infinite loops, stack overflows, or excessive memory allocation in the
  parser, optimizer, or backends.
- **Soundness holes in `unsafe` JIT code** — issues in `mmap`/`mprotect` usage,
  function pointer transmutes, or other `unsafe` blocks that can be triggered by
  a caller with normal (non-privileged) API usage.

### Out of scope

- Vulnerabilities in Rust's standard library or the `rustc` compiler itself.
  Report those to the [Rust Security Team](https://www.rust-lang.org/policies/security).
- Issues that require the attacker to supply arbitrary LLVM bitcode or `.ll`
  files to a deployment that intentionally accepts such input. This library is
  not a sandbox; callers are responsible for validating untrusted IR before
  passing it through the pipeline.
- Pure performance regressions with no security impact (file a regular issue).
- Theoretical issues with no demonstrated exploit path.

---

## Security-Sensitive Areas

The following modules use `unsafe` Rust or perform low-level byte manipulation.
They are particularly worth careful review when auditing the codebase:

- **`llvm-jit/src/lib.rs`** — `mmap` + `mprotect` calls and function-pointer
  transmutes for JIT-compiled code execution. Any bug here can lead to arbitrary
  code execution in the host process.
- **`llvm-codegen/src/emit.rs`** — raw byte encoding of machine instructions
  into ELF/Mach-O/COFF object files. Incorrect encoding could produce object
  files with malformed sections or relocations.
- **Parser inputs from untrusted sources** — `llvm-ir-parser` processes `.ll`
  text files. Although the parser is written in safe Rust, very large or
  adversarially crafted inputs may trigger pathological parse times or excessive
  memory use. Apply input size limits before passing untrusted files.
