#!/usr/bin/env bash
set -euo pipefail

echo "=== Host CPU / LSE probe ==="
lscpu | grep -E 'Model name|Architecture|Flags|CPU\(s\)|MHz' || true
head -40 /proc/cpuinfo | grep -E 'model name|CPU implementer|CPU architecture|CPU variant|CPU part|Features' || true

ARCH=$(uname -m)
[[ "$ARCH" == "aarch64" ]] || { echo "ERROR: not running on aarch64 (got $ARCH)"; exit 1; }

FEATS=$(awk -F: '/^Features/{gsub(/ /,",$2); print $2}' /proc/cpuinfo)
echo "CPU features: $FEATS"

if echo "$FEATS" | grep -qw lse; then
  echo "LSE atomics: supported"
  LSE=true
else
  echo "LSE atomics: NOT supported — using armv8-a baseline"
  LSE=false
fi

# clang march test
if command -v clang &>/dev/null; then
  cat > /tmp/march_test.c <<'EOF'
#include <stdio.h>
int main(void) { printf("march-ok\n"); return 0; }
EOF
  if [ "$LSE" = true ]; then
    clang -march=native -o /tmp/march_test /tmp/march_test.c && /tmp/march_test
  else
    clang -march=armv8-a -o /tmp/march_test /tmp/march_test.c && /tmp/march_test
  fi
  echo "clang march test: OK"
fi

# Set safe RUSTFLAGS based on LSE availability
if [ "$LSE" = true ]; then
  export RUSTFLAGS="-C target-cpu=native -C target-feature=+lse -C codegen-units=1"
else
  export RUSTFLAGS="-C target-feature=-lse -C target-feature=-rcpc -C codegen-units=1"
fi
echo "RUSTFLAGS: $RUSTFLAGS"

echo "=== cargo check (aarch64 target) ==="
cargo check -p llvm-in-rust-target-arm --target aarch64-unknown-linux-gnu

echo "=== platform_matrix target-aarch64 ==="
scripts/platform_matrix.sh target-aarch64

echo "=== cargo build --release ==="
cargo build --release

echo "=== cargo test ==="
cargo test

echo "=== clippy + fmt ==="
cargo clippy --all-targets
cargo fmt --check

echo "=== DONE ==="
ls -lh target/release/ 2>/dev/null | head -10
