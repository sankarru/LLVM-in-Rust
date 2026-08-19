#!/usr/bin/env bash
set -euo pipefail

echo "=== Host CPU / -march probe ==="
lscpu | grep -E 'Model name|Architecture|Flags|CPU\(s\)|MHz' || true
head -40 /proc/cpuinfo | grep -E 'model name|CPU implementer|CPU architecture|CPU variant|CPU part|Features' || true

ARCH=$(uname -m)
[[ "$ARCH" == "aarch64" ]] || { echo "ERROR: not running on aarch64 (got $ARCH)"; exit 1; }

# clang -march=native test
if command -v clang &>/dev/null; then
  cat > /tmp/march_test.c <<'EOF'
#include <stdio.h>
int main(void) { printf("march-ok\n"); return 0; }
EOF
  clang -march=native -o /tmp/march_test /tmp/march_test.c && /tmp/march_test
  echo "clang -march=native: OK"
fi

echo "=== cargo check (aarch64 target) ==="
cargo check -p llvm-in-rust-target-arm --target aarch64-unknown-linux-gnu

echo "=== platform_matrix target-aarch64 ==="
scripts/platform_matrix.sh target-aarch64

echo "=== cargo build --release (target-cpu=native) ==="
RUSTFLAGS="-C target-cpu=native -C codegen-units=1" cargo build --release

echo "=== cargo test ==="
cargo test

echo "=== clippy + fmt ==="
cargo clippy --all-targets
cargo fmt --check

echo "=== DONE ==="
echo "Binary: target/release/"
ls -lh target/release/ 2>/dev/null | head -10
