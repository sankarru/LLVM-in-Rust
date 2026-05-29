#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'USAGE'
Usage: scripts/backend_support_contract.sh <lane>

Lanes:
  validate     Validate docs/backend_support_matrix.json shape and evidence paths.
  rustc-smoke  Run release-blocking stable rustc-backend shim tests.
  lto-smoke    Run release-blocking LTO smoke tests.
  release      Run all lanes used by the Backend Support Contract workflow.
USAGE
}

lane="${1:-}"
if [[ -z "$lane" || "$lane" == "-h" || "$lane" == "--help" ]]; then
  usage
  exit 0
fi

validate() {
  python3 scripts/backend_support_contract.py
}

rustc_smoke() {
  cargo +stable test -p llvm-in-rust-rustc-backend
}

lto_smoke() {
  cargo +stable test -p llvm-in-rust --test lto_multi_tu
  cargo +stable test -p llvm-in-rust-codegen --test thinlto
}

case "$lane" in
  validate) validate ;;
  rustc-smoke) rustc_smoke ;;
  lto-smoke) lto_smoke ;;
  release)
    validate
    rustc_smoke
    lto_smoke
    ;;
  *)
    usage >&2
    exit 2
    ;;
esac
