#!/usr/bin/env bash
# bench/real-programs/run.sh
#
# Measures the runtime quality of our codegen vs clang -O2 on three scalar
# benchmarks.  For each benchmark:
#   - reference : clang -O2 compiles the .c fixture
#   - ours      : llvm-ir-compile reads the hand-crafted .ll fixture, links with cc
#
# The .ll files are written in the same style as the smoke tests so they are
# fully handled by our pipeline (mem2reg promotes all allocas to SSA).
#
# Prerequisites
#   - Rust toolchain (cargo) on PATH
#   - clang on PATH
#   - cc (system linker) on PATH
#
# Usage (from workspace root):
#   bash bench/real-programs/run.sh

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WORKSPACE_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"
FIXTURES="${SCRIPT_DIR}/fixtures"
TMPDIR_BENCH="${SCRIPT_DIR}/.tmp"
DRIVER="${WORKSPACE_ROOT}/target/release/llvm-ir-compile"

RED='\033[0;31m'; GREEN='\033[0;32m'; BOLD='\033[1m'; RESET='\033[0m'

# ── step 1: build the driver ──────────────────────────────────────────────────
echo -e "${BOLD}Building llvm-ir-compile (release)...${RESET}"
cargo build --release -p llvm --manifest-path "${WORKSPACE_ROOT}/Cargo.toml"
echo ""

mkdir -p "${TMPDIR_BENCH}"

# ── helpers ───────────────────────────────────────────────────────────────────

# median_time <binary>  → prints wall-clock seconds (3-run median)
median_time() {
    local bin="$1"
    local times=()
    for _ in 1 2 3; do
        local t
        t=$({ time "$bin" > /dev/null 2>&1; } 2>&1 \
            | grep real \
            | awk '{gsub(/[ms]/," ",$2); split($2,a," "); printf "%.3f", a[1]*60+a[2]}')
        times+=("$t")
    done
    printf '%s\n' "${times[@]}" | sort -n | sed -n '2p'
}

declare -a RESULTS=()

run_bench() {
    local name="$1"
    local c_src="${FIXTURES}/bench_${name}.c"
    local ll_src="${FIXTURES}/bench_${name}.ll"
    local ref_bin="${TMPDIR_BENCH}/bench_${name}_ref"
    local our_obj="${TMPDIR_BENCH}/bench_${name}.o"
    local our_bin="${TMPDIR_BENCH}/bench_${name}_ours"

    echo -e "${BOLD}── ${name} ──────────────────────────────────${RESET}"

    # (a) reference: clang -O2
    echo "  [ref]  clang -O2 ${c_src##*/} ..."
    clang -O2 -o "${ref_bin}" "${c_src}"

    # (b) ours: llvm-ir-compile reads hand-crafted .ll
    echo "  [ours] llvm-ir-compile ${ll_src##*/} ..."
    if ! "${DRIVER}" "${ll_src}" -o "${our_obj}" 2>&1; then
        echo -e "  ${RED}COMPILE FAILED — skipping${RESET}"
        return
    fi

    echo "  [ours] linking ..."
    if ! cc "${our_obj}" -o "${our_bin}" 2>&1 | grep -v "^ld: warning:"; then
        true  # link warnings are OK
    fi
    if [ ! -f "${our_bin}" ]; then
        echo -e "  ${RED}LINK FAILED — skipping${RESET}"
        return
    fi

    # (c) correctness: compare exit codes
    set +e
    "${ref_bin}" > /dev/null 2>&1; ref_code=$?
    "${our_bin}" > /dev/null 2>&1; our_code=$?
    set -e

    if [ "${ref_code}" -eq "${our_code}" ]; then
        correct_label="${GREEN}OK (exit=${our_code})${RESET}"
    else
        correct_label="${RED}FAIL (ref=${ref_code} ours=${our_code})${RESET}"
    fi

    # (d) timing
    echo "  Timing (3 runs each) ..."
    ref_time=$(median_time "${ref_bin}")
    our_time=$(median_time "${our_bin}")

    # (e) slowdown ratio
    ratio=$(awk "BEGIN {printf \"%.1fx\", ${our_time}/${ref_time}}")

    # (f) binary sizes
    ref_size=$(wc -c < "${ref_bin}" | tr -d ' ')
    our_size=$(wc -c < "${our_bin}" | tr -d ' ')

    printf "  %-10s  ref: %6ss  ours: %6ss  slowdown: %6s  sizes: %7d / %7d  correct: " \
        "${name}" "${ref_time}" "${our_time}" "${ratio}" "${ref_size}" "${our_size}"
    echo -e "${correct_label}"

    RESULTS+=("${name}|${ref_time}|${our_time}|${ratio}|${ref_size}|${our_size}|${ref_code}|${our_code}")
}

# ── step 2: run benchmarks ────────────────────────────────────────────────────
for bench in fib gcd collatz; do
    run_bench "${bench}" || true
    echo ""
done

# ── step 3: summary table ─────────────────────────────────────────────────────
echo -e "${BOLD}╔══════════╤════════════╤════════════╤══════════╤═════════════╤═════════════╤═════════════╗${RESET}"
echo -e "${BOLD}║ program  │ clang-O2 s │  ours s    │ slowdown │  ref bytes  │  ours bytes │ correct?    ║${RESET}"
echo -e "${BOLD}╠══════════╪════════════╪════════════╪══════════╪═════════════╪═════════════╪═════════════╣${RESET}"
for row in "${RESULTS[@]:-}"; do
    [ -z "${row}" ] && continue
    IFS='|' read -r name ref_t our_t ratio ref_sz our_sz ref_c our_c <<< "${row}"
    ok=$([[ "${ref_c}" -eq "${our_c}" ]] && echo "OK (${our_c})" || echo "FAIL ref=${ref_c} ours=${our_c}")
    printf "║ %-8s │ %10s │ %10s │ %8s │ %11s │ %11s │ %-11s ║\n" \
        "${name}" "${ref_t}" "${our_t}" "${ratio}" "${ref_sz}" "${our_sz}" "${ok}"
done
echo -e "${BOLD}╚══════════╧════════════╧════════════╧══════════╧═════════════╧═════════════╧═════════════╝${RESET}"
echo ""
echo "Artifacts left in: ${TMPDIR_BENCH}"
echo "Done."
