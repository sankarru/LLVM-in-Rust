#!/usr/bin/env bash
# bench/real-programs/run.sh
# Benchmark driver: compile each fixture with clang -O2 (reference) and with
# our llvm-ir-compile pipeline (ours), time both, check correctness, compare sizes.
#
# Prerequisites:
#   - Rust toolchain (cargo) in PATH
#   - clang in PATH
#   - cc (system linker) in PATH
#
# Usage: bash bench/real-programs/run.sh
# (Run from the workspace root, i.e., the directory containing Cargo.toml)

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WORKSPACE_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"
FIXTURES="${SCRIPT_DIR}/fixtures"
TMPDIR="${SCRIPT_DIR}/.tmp"
DRIVER="${WORKSPACE_ROOT}/target/release/llvm-ir-compile"

# ── colours ──────────────────────────────────────────────────────────────────
RED='\033[0;31m'; GREEN='\033[0;32m'; YELLOW='\033[1;33m'
BOLD='\033[1m'; RESET='\033[0m'

# ── step 1: build the driver ─────────────────────────────────────────────────
echo -e "${BOLD}Building llvm-ir-compile (release)...${RESET}"
cargo build --release -p llvm --manifest-path "${WORKSPACE_ROOT}/Cargo.toml"
echo ""

mkdir -p "${TMPDIR}"
cd "${TMPDIR}"

# ── helpers ───────────────────────────────────────────────────────────────────

# time_run <binary> → prints real time in seconds to stdout, exit code to stderr
time_run() {
    local bin="$1"
    # Use bash time builtin with a specific output format
    local time_output
    time_output=$({ time "$bin"; } 2>&1)
    # Extract real time from "real  0m1.234s" or "real 1.234" depending on shell
    echo "$time_output" | grep real | awk '{print $2}' | sed 's/[ms]/ /g' | awk '{printf "%.3f\n", $1*60+$2}'
}

run_bench() {
    local name="$1"
    local src="${FIXTURES}/bench_${name}.c"

    echo -e "${BOLD}── ${name} ──────────────────────────────────${RESET}"

    # (a) clang -O2 reference binary
    echo "  [ref]   clang -O2 ..."
    clang -O2 -o "bench_${name}_ref" "${src}"

    # (b) emit LLVM IR with clang -O0
    echo "  [ours]  clang -O0 -S -emit-llvm ..."
    clang -O0 -S -emit-llvm "${src}" -o "bench_${name}.ll"

    # (c) compile with our pipeline
    echo "  [ours]  llvm-ir-compile ..."
    if ! "${DRIVER}" "bench_${name}.ll" -o "bench_${name}.o" 2>&1; then
        echo -e "  ${RED}COMPILE FAILED${RESET}"
        return 1
    fi

    # (d) link
    echo "  [ours]  link ..."
    if ! cc "bench_${name}.o" -o "bench_${name}_ours" 2>&1; then
        echo -e "  ${RED}LINK FAILED${RESET}"
        return 1
    fi

    # (e) time both — run each binary 3 times and take the median
    echo "  Timing (3 runs each)..."

    local ref_times=()
    local our_times=()
    local ref_exit our_exit

    for run in 1 2 3; do
        local t
        t=$( { time "./bench_${name}_ref" > /dev/null; } 2>&1 | grep real | awk '{print $2}' | sed 's/m/ /; s/s//' | awk '{printf "%.3f", $1*60+$2}' )
        ref_exit=$?
        ref_times+=("$t")
    done

    for run in 1 2 3; do
        local t
        t=$( { time "./bench_${name}_ours" > /dev/null; } 2>&1 | grep real | awk '{print $2}' | sed 's/m/ /; s/s//' | awk '{printf "%.3f", $1*60+$2}' )
        our_exit=$?
        our_times+=("$t")
    done

    # sort and take middle value
    local ref_sorted=( $(printf '%s\n' "${ref_times[@]}" | sort -n) )
    local our_sorted=( $(printf '%s\n' "${our_times[@]}" | sort -n) )
    local ref_time="${ref_sorted[1]}"
    local our_time="${our_sorted[1]}"

    # (f) get exit codes for correctness
    local ref_code our_code
    set +e
    "./bench_${name}_ref" > /dev/null 2>&1; ref_code=$?
    "./bench_${name}_ours" > /dev/null 2>&1; our_code=$?
    set -e

    local correct_label
    if [ "$ref_code" -eq "$our_code" ]; then
        correct_label="${GREEN}OK (exit=$our_code)${RESET}"
    else
        correct_label="${RED}FAIL (ref=$ref_code ours=$our_code)${RESET}"
    fi

    # (g) binary sizes
    local ref_size our_size slowdown ratio_str
    ref_size=$(wc -c < "bench_${name}_ref" | tr -d ' ')
    our_size=$(wc -c < "bench_${name}_ours" | tr -d ' ')

    # compute slowdown ratio (ours/ref) using awk
    if [ -n "$ref_time" ] && [ -n "$our_time" ]; then
        ratio_str=$(awk "BEGIN {printf \"%.1fx\", ${our_time}/${ref_time}}")
    else
        ratio_str="N/A"
    fi

    # print result row
    printf "  %-12s  ref: %6ss  ours: %6ss  slowdown: %6s  sizes: %7d / %7d bytes  correctness: " \
        "${name}" "${ref_time:-?}" "${our_time:-?}" "${ratio_str}" "${ref_size}" "${our_size}"
    echo -e "${correct_label}"

    # store results for summary
    RESULTS+=( "${name}|${ref_time:-?}|${our_time:-?}|${ratio_str}|${ref_size}|${our_size}|${ref_code}|${our_code}" )
}

# ── step 2: run each benchmark ────────────────────────────────────────────────

declare -a RESULTS=()

for bench in fib gcd collatz; do
    run_bench "${bench}" || true
    echo ""
done

# ── step 3: summary table ─────────────────────────────────────────────────────

echo -e "${BOLD}╔══════════════╤════════════╤════════════╤══════════╤═════════════╤═════════════╤═════════════╗${RESET}"
echo -e "${BOLD}║ program      │ clang-O2 s │  ours s    │ slowdown │  ref bytes  │  ours bytes │ correct?    ║${RESET}"
echo -e "${BOLD}╠══════════════╪════════════╪════════════╪══════════╪═════════════╪═════════════╪═════════════╣${RESET}"

for row in "${RESULTS[@]}"; do
    IFS='|' read -r name ref_t our_t ratio ref_sz our_sz ref_code our_code <<< "${row}"
    if [ "$ref_code" -eq "$our_code" ]; then
        ok_str="OK ($our_code)"
    else
        ok_str="FAIL ref=$ref_code ours=$our_code"
    fi
    printf "║ %-12s │ %10s │ %10s │ %8s │ %11s │ %11s │ %-11s ║\n" \
        "${name}" "${ref_t}" "${our_t}" "${ratio}" "${ref_sz}" "${our_sz}" "${ok_str}"
done

echo -e "${BOLD}╚══════════════╧════════════╧════════════╧══════════╧═════════════╧═════════════╧═════════════╝${RESET}"

echo ""
echo "Artifacts left in: ${TMPDIR}"
echo "Done."
