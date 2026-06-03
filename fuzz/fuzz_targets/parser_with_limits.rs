//! Fuzz target: parse adversarial LLVM IR under the `production_defaults`
//! resource limits.
//!
//! Milestone X / issue #383: ensures `parse_with_limits` itself does not
//! panic, hang, or consume unbounded resources on arbitrary input — beyond
//! what the existing un-limited `parser.rs` target covers.  The limit checks
//! are the new code path under test here; the cap profile must produce a
//! structured [`ParseError`] (not a panic) on every overflow.
//!
//! Build (host with `cargo-fuzz`):
//!   cargo +nightly fuzz run parser_with_limits -- -runs=10000 -max_len=131072

#![no_main]

use libfuzzer_sys::fuzz_target;
use llvm_ir_parser::parser::{parse_with_limits, ParseLimits};

fuzz_target!(|data: &[u8]| {
    // Don't waste fuzzer cycles on absurdly large inputs — the source-bytes
    // limit will reject them anyway, and libFuzzer corpus minimization should
    // not preserve them.
    if data.len() > 1 << 17 {
        return;
    }

    // Non-UTF-8 inputs aren't valid LLVM IR text; skip without panicking.
    let src = match std::str::from_utf8(data) {
        Ok(s) => s,
        Err(_) => return,
    };

    // The contract under test: `parse_with_limits` returns `Result`, never
    // panics, never hangs.  Any panic here is a real bug.
    let _ = parse_with_limits(src, ParseLimits::production_defaults());
});
