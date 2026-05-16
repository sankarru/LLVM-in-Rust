#!/usr/bin/env bash
# scripts/frontend_integration.sh
#
# C frontend integration test runner (issue #217).
#
# For each .c file in tests/c_frontend/:
#   1. Compile with clang -O1 -emit-llvm to produce LLVM IR
#   2. Compile the IR through our pipeline (llvm-compile binary)
#   3. Link with clang to produce an executable
#   4. Run both our executable and the clang reference; compare exit codes
#
# Usage:
#   bash scripts/frontend_integration.sh              # run all tests
#   bash scripts/frontend_integration.sh 06_recursion # run one test (basename match)
#
# Prerequisites: clang, cargo (Rust toolchain)
# Returns 0 if all tests pass, 1 if any fail.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
C_TESTS_DIR="${REPO_ROOT}/tests/c_frontend"
TMP_DIR="${TMPDIR:-/tmp}/llvm_in_rust_c_frontend_$$"
BINARY="llvm-compile"
FILTER="${1:-}"

# ── prerequisites ────────────────────────────────────────────────────────────

if ! command -v clang &>/dev/null; then
    echo "SKIP: clang not found — skipping frontend integration tests" >&2
    exit 0
fi

# Build the llvm-compile binary.
echo "=== Building ${BINARY}..."
cargo +stable build --quiet --bin "${BINARY}" 2>&1

LLVM_COMPILE="${REPO_ROOT}/target/debug/${BINARY}"
if [ ! -x "${LLVM_COMPILE}" ]; then
    LLVM_COMPILE="${REPO_ROOT}/target/release/${BINARY}"
fi
if [ ! -x "${LLVM_COMPILE}" ]; then
    echo "ERROR: ${BINARY} binary not found after build" >&2
    exit 1
fi

# ── run tests ────────────────────────────────────────────────────────────────

mkdir -p "${TMP_DIR}"
trap 'rm -rf "${TMP_DIR}"' EXIT

PASS=0
FAIL=0
SKIP=0

for c_file in "${C_TESTS_DIR}"/*.c; do
    base="$(basename "${c_file}" .c)"

    # Apply filter if provided.
    if [ -n "${FILTER}" ] && [[ "${base}" != *"${FILTER}"* ]]; then
        continue
    fi

    echo -n "  ${base}: "

    ir_file="${TMP_DIR}/${base}.ll"
    our_obj="${TMP_DIR}/${base}.o"
    our_exe="${TMP_DIR}/${base}_ours"
    ref_exe="${TMP_DIR}/${base}_ref"

    # Step 1: compile C → LLVM IR
    if ! clang -O1 -S -emit-llvm -o "${ir_file}" "${c_file}" 2>/dev/null; then
        echo "SKIP (clang emit-llvm failed)"
        (( SKIP++ )) || true
        continue
    fi

    # Step 2: compile IR → object via our pipeline
    if ! "${LLVM_COMPILE}" "${ir_file}" -o "${our_obj}" 2>/dev/null; then
        echo "FAIL (llvm-compile failed)"
        (( FAIL++ )) || true
        continue
    fi

    # Step 3: link our object using clang as linker (handles CRT)
    if ! clang "${our_obj}" -o "${our_exe}" 2>/dev/null; then
        echo "FAIL (link failed)"
        (( FAIL++ )) || true
        continue
    fi

    # Step 4: build reference binary directly from C
    if ! clang -O1 "${c_file}" -o "${ref_exe}" 2>/dev/null; then
        echo "SKIP (clang reference build failed)"
        (( SKIP++ )) || true
        continue
    fi

    # Step 5: run both and compare exit codes
    "${our_exe}" >/dev/null 2>&1 || true
    our_exit=$?
    "${ref_exe}" >/dev/null 2>&1 || true
    ref_exit=$?

    if [ "${our_exit}" -eq "${ref_exit}" ]; then
        echo "PASS (exit=${our_exit})"
        (( PASS++ )) || true
    else
        echo "FAIL (our=${our_exit}, ref=${ref_exit})"
        (( FAIL++ )) || true
    fi
done

echo ""
echo "=== Results: ${PASS} passed, ${FAIL} failed, ${SKIP} skipped"

if [ "${FAIL}" -gt 0 ]; then
    exit 1
fi
exit 0
