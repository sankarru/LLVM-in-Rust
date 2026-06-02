#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DOC="$ROOT/docs/production_operations.md"
SUPPORT_DOC="$ROOT/docs/production_support_boundaries.md"
SANDBOX_DOC="$ROOT/docs/sandbox_deployment.md"
README="$ROOT/README.md"
CHANGELOG="$ROOT/CHANGELOG.md"
LICENSE_FILE="$ROOT/LICENSE"
MANIFEST="$ROOT/Cargo.toml"

require_in_doc() {
  local needle="$1"
  if ! grep -Fq -- "$needle" "$DOC"; then
    echo "production operations guide missing required text: $needle" >&2
    exit 1
  fi
}

require_in_readme() {
  local needle="$1"
  if ! grep -Fq -- "$needle" "$README"; then
    echo "README missing production operations link/text: $needle" >&2
    exit 1
  fi
}

require_in_file() {
  local file="$1"
  local needle="$2"
  if ! grep -Fq -- "$needle" "$file"; then
    echo "$(basename "$file") missing required text: $needle" >&2
    exit 1
  fi
}

reject_in_file() {
  local file="$1"
  local needle="$2"
  if grep -Fq -- "$needle" "$file"; then
    echo "$(basename "$file") contains stale text: $needle" >&2
    exit 1
  fi
}

[[ -f "$DOC" ]] || { echo "missing $DOC" >&2; exit 1; }
[[ -f "$SUPPORT_DOC" ]] || { echo "missing $SUPPORT_DOC" >&2; exit 1; }
[[ -f "$SANDBOX_DOC" ]] || { echo "missing $SANDBOX_DOC" >&2; exit 1; }

require_in_doc "## Build and validation quick start"
require_in_doc "## Observability checklist"
require_in_doc "## Incident response: start to resolution"
require_in_doc "## Contributor triage paths"
require_in_doc "## FAQ: common integration failures"
require_in_doc "## Runbook index"
require_in_doc "scripts/reduce_ci_failure.sh"
require_in_doc "scripts/release_artifacts.sh verify"
require_in_doc "docs/production_support_boundaries.md"
require_in_doc "docs/sandbox_deployment.md"
require_in_doc "docs/release_candidate_protocol.md"
require_in_doc "docs/crash_triage_runbook.md"
require_in_readme "docs/production_operations.md"
require_in_readme "docs/production_support_boundaries.md"
require_in_readme "docs/sandbox_deployment.md"

require_in_file "$SUPPORT_DOC" "## Production Scope"
require_in_file "$SUPPORT_DOC" "## Pre-1.0 API Stability Matrix"
require_in_file "$SUPPORT_DOC" "## Backend and Platform Boundaries"
require_in_file "$SUPPORT_DOC" "## Untrusted Input Deployment"
require_in_file "$SUPPORT_DOC" "| x86-64 native backend |"
require_in_file "$SUPPORT_DOC" "| WebAssembly backend |"
require_in_file "$SUPPORT_DOC" "| Known unsupported cases |"
require_in_file "$CHANGELOG" "## [0.1.0] - 2026-05-13"
require_in_file "$LICENSE_FILE" "APPENDIX: How to apply the Apache License to your work."
require_in_file "$MANIFEST" "[workspace.package]"
require_in_file "$MANIFEST" 'license = "Apache-2.0"'
require_in_file "$MANIFEST" 'repository = "https://github.com/yudongusa/LLVM-in-Rust"'
require_in_file "$SANDBOX_DOC" "## Security Model"
require_in_file "$SANDBOX_DOC" "## Minimum Isolation Contract"
require_in_file "$SANDBOX_DOC" "## Resource Limits"
require_in_file "$SANDBOX_DOC" "## Platform Patterns"
require_in_file "$SANDBOX_DOC" "## JIT-Specific Rules"
require_in_file "$SANDBOX_DOC" "## Temporary Directory Hygiene"
require_in_file "$SANDBOX_DOC" "## Pilot Sign-Off Checklist"
require_in_file "$SANDBOX_DOC" "seccomp"
require_in_file "$SANDBOX_DOC" "sandbox-exec"
require_in_file "$SANDBOX_DOC" "JIT is disabled"
require_in_file "$SANDBOX_DOC" "parse_with_limits"
require_in_file "$SANDBOX_DOC" "--production-parse-limits"

reject_in_file "$README" "523 tests"
reject_in_file "$README" "1,076 tests"
reject_in_file "$README" '| `freeze` instruction | **No**'
reject_in_file "$README" '| `vp.*` vector-predication intrinsics | **No**'
reject_in_file "$CHANGELOG" "## [0.1.0] - Unreleased"

echo "production operations docs are complete"
