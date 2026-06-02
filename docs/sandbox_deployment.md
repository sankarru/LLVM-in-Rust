# Sandbox Deployment Guidance

Issue #383 (Milestone X) requires an explicit security model for deployments
that may receive malformed or adversarial IR, LRIR, object, or source-derived
inputs.

## Security Model

LLVM-in-Rust is a compiler toolchain, not a sandbox. The parser and bitcode
readers return structured errors for malformed input, but callers must still
bound CPU, memory, filesystem, and executable-memory exposure when inputs are
not fully trusted.

| Input source | Required posture |
|---|---|
| Build artifacts produced by a trusted internal compiler pipeline | May run in-process when pinned to a validated commit and covered by the production support boundaries. |
| User-uploaded `.ll`, `.bc`, LRIR, object files, or source that is converted into IR | Run out-of-process with resource limits and filesystem isolation. |
| Fuzzing, reducer, compatibility-suite, or benchmark inputs | Run in disposable workers with timeouts and artifact quotas. |
| JIT execution from anything other than trusted IR | Disable JIT, or isolate it in a short-lived process/container with executable-memory policy controls. |

## Minimum Isolation Contract

Use a worker process boundary for untrusted inputs. The host service should:

1. Spawn a fresh worker per request or per small batch.
2. Pass inputs through a private temporary directory.
3. Apply wall-clock, CPU, memory, file-size, process-count, and open-file
   limits before invoking parser, optimizer, codegen, object emission, or JIT
   paths.
4. Deny network access unless the specific pipeline stage requires it.
5. Mount the workspace read-only and write outputs only to a request-scoped
   directory.
6. Delete temporary directories after collecting the minimal diagnostic
   bundle.
7. Treat a timeout, signal, or resource-limit exit as an expected rejected
   input, not as a host-service crash.

The library APIs are suitable inside the worker process. Do not expose those
APIs directly inside a long-lived control-plane process when the input is
untrusted.

## Resource Limits

Apply host-level limits even when using parser or optimizer limit knobs.
In-process limits protect algorithmic code paths; OS/container limits protect
the embedding application.

| Resource | Guidance |
|---|---|
| Wall clock | Set a request timeout and kill the worker on expiry. Use tighter limits for parser-only validation and broader limits for full codegen or compatibility-suite runs. |
| CPU | Use `RLIMIT_CPU`, cgroups, Kubernetes CPU limits, or the platform equivalent. Avoid unbounded optimizer fixed-point loops on user inputs. |
| Memory | Use cgroup memory limits, container memory limits, or `ulimit -v`/`RLIMIT_AS` where available. Record OOM exits distinctly from parse errors. |
| Output size | Cap object, assembly, log, reducer, and benchmark artifact sizes. Refuse to archive unbounded stdout/stderr. |
| File descriptors and processes | Apply `RLIMIT_NOFILE` and `RLIMIT_NPROC`/container process limits. |
| Temporary storage | Use a request-scoped temp directory with a quota. Never reuse temp paths across tenants. |
| Recursion and nesting | Use `parse_with_limits` / `ParseLimits` or the `llvm-compile` parse-limit flags for source size, function count, block/instruction count, and type/constant nesting. Keep worker timeouts and memory caps as the outer guard. |

Parser limits are opt-in so trusted existing callers keep historical behavior.
For untrusted LLVM text IR, use the library API:

```rust
use llvm_ir_parser::parser::{parse_with_limits, ParseLimits};

let limits = ParseLimits::production_defaults();
let (_ctx, _module) = parse_with_limits(input, limits)?;
```

For the `llvm-compile` CLI, use the production preset or override individual
limits:

```bash
llvm-compile input.ll -o output.o \
  --production-parse-limits \
  --max-input-bytes 16777216 \
  --max-functions 10000 \
  --max-blocks-per-function 100000 \
  --max-instructions-per-function 1000000 \
  --max-type-depth 128 \
  --max-constant-depth 128
```

## Platform Patterns

### Linux

Prefer containers or a service-level sandbox with cgroups and seccomp:

```bash
docker run --rm \
  --network=none \
  --memory=512m \
  --cpus=1 \
  --pids-limit=64 \
  --read-only \
  --tmpfs /tmp:rw,noexec,nosuid,nodev,size=64m \
  -v "$PWD/input:/input:ro" \
  -v "$PWD/out:/out:rw" \
  llvm-in-rust-worker:latest \
  llvm-ir-min /input/repro.ll --output /out/min.ll
```

For native services, combine cgroups with a seccomp profile that denies
network, process-spawning, and filesystem mutation outside the worker
directory. Keep the exact allowed syscalls tied to the worker binary and
revalidate after dependency or runtime changes.

### macOS

Use a short-lived worker process and `sandbox-exec` or an equivalent managed
sandbox profile when available:

```bash
sandbox-exec -p '(version 1)
  (deny default)
  (allow file-read* (subpath "/path/to/input"))
  (allow file-write* (subpath "/path/to/out"))
  (allow file-write* (subpath "/private/tmp/llvm-in-rust-worker"))
  (allow process*)
  (deny network*)' \
  llvm-ir-min /path/to/input/repro.ll --output /path/to/out/min.ll
```

Also apply an outer timeout from the orchestrator, because `sandbox-exec` does
not by itself bound CPU or memory.

### Windows

Run untrusted workloads in a constrained job object, container, or VM. Set job
object limits for process count, working set/memory, CPU time, and active
process lifetime. Write outputs to a request-scoped directory with inherited
ACLs that exclude unrelated service data.

## JIT-Specific Rules

The JIT allocates executable memory. For hostile input, the safest default is
to disable JIT entirely and use object emission plus external validation in a
disposable worker.

If a pilot must JIT untrusted or tenant-controlled IR:

- run the JIT in a separate process/container with no ambient credentials;
- enforce W^X / executable-memory policy where the host supports it;
- disable network and restrict filesystem access;
- cap the number and size of compiled modules;
- terminate the worker after the JIT call completes;
- never call JIT-produced function pointers in the control-plane process.

## Temporary Directory Hygiene

Use a fresh directory for each request:

```text
/var/tmp/llvm-in-rust/<request-id>/
  input/
  work/
  output/
  logs/
```

Requirements:

- Generate request IDs with sufficient entropy; do not use user-provided file
  names as directory names.
- Open files with no-follow semantics where practical.
- Normalize and reject paths that escape the request root.
- Store the original input checksum and sanitized file name in metadata.
- Delete the directory after success, or retain it only behind an explicit
  incident-retention policy.

## Operational Handling

For every rejected or timed-out input, record:

- commit SHA, crate/binary version, and Rust toolchain;
- input checksum, producer, and size;
- resource limits applied;
- command/API surface invoked;
- exit status, signal, timeout, stderr, and backtrace if available;
- path to the minimized reproducer when one is created.

Use `docs/crash_triage_runbook.md` for crash, timeout, and miscompilation
triage. Use `docs/release_candidate_protocol.md` if a production pilot or RC
run hits a release-blocking failure.

## Pilot Sign-Off Checklist

Before claiming support for an untrusted-input deployment:

- [ ] Worker process/container boundary is mandatory for all untrusted inputs.
- [ ] CPU, memory, wall-clock, process-count, file-size, and temp-storage
      limits are enforced outside the Rust process.
- [ ] Network access is disabled unless explicitly required.
- [ ] JIT is disabled, or JIT runs only inside a disposable worker with
      executable-memory policy controls.
- [ ] Temp directories are request-scoped, non-shared, and cleaned up.
- [ ] Error/timeout/OOM outcomes are observable and do not crash the host
      service.
- [ ] Fallback to upstream LLVM or a known-good artifact path is documented.

Refs #93, refs #383.
