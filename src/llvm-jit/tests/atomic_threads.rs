//! Multithreaded spinlock smoke test for `atomicrmw add` via JIT.
//!
//! Parses a tiny IR snippet containing `atomicrmw add ptr %ctr, i32 1 seq_cst`,
//! JIT-compiles it through the full pipeline (parse → O2 → x86 regalloc → mmap),
//! then drives two or more OS threads against the resulting function pointer and
//! checks that no increment is lost.
//!
//! Only runs on x86-64 because `SimpleJit` only emits x86-64 machine code.

// Everything in this file is x86-64-only: gate the entire module contents so
// the compiler doesn't warn about unused items on other architectures.
#[cfg(target_arch = "x86_64")]
mod x86_64_tests {
    use llvm_ir_parser::parser::parse;
    use llvm_jit::{ExecutionEngine, SimpleJit};
    use std::sync::atomic::{AtomicI32, Ordering};
    use std::sync::Arc;

    const N: i32 = 500;

    /// IR for `void @increment(ptr %ctr)` — atomically adds 1 to the pointed-to i32.
    const SPINLOCK_IR: &str = r#"
define void @increment(ptr %ctr) {
entry:
  %old = atomicrmw add ptr %ctr, i32 1 seq_cst
  ret void
}
"#;

    // ── helper ──────────────────────────────────────────────────────────────

    /// Parse `SPINLOCK_IR`, JIT-compile it, and return the engine.
    ///
    /// Separated into a helper so both tests can share the setup logic without
    /// duplicating the parse/compile boilerplate.
    fn build_jit() -> SimpleJit {
        let (mut ctx, mut module) = parse(SPINLOCK_IR).expect("parse failed");
        let mut jit = SimpleJit::new();
        jit.add_module(&mut ctx, &mut module)
            .expect("JIT compilation failed");
        jit
    }

    // ── tests ────────────────────────────────────────────────────────────────

    /// Two threads each call `increment` N times; final counter must equal 2 × N.
    ///
    /// Verifies that the JIT-compiled `atomicrmw add seq_cst` produces a real
    /// atomic LOCK XADD instruction that prevents lost updates under contention.
    #[test]
    fn multithreaded_atomic_counter_no_lost_updates() {
        let jit = build_jit();

        let ptr = jit.get_function_ptr("increment").expect("symbol not found");

        // Shared counter — heap-allocated so both threads can hold a raw pointer.
        // The Arc keeps the allocation alive until after both threads have joined.
        let counter = Arc::new(AtomicI32::new(0));
        let ctr_ptr = counter.as_ref() as *const AtomicI32 as *mut i32;

        // SAFETY: `ptr` points to JIT-compiled x86-64 `void @increment(ptr %ctr)`.
        // The `SimpleJit` owns the mmap pages and outlives all threads in this
        // function (it is not moved into the threads). `ctr_ptr` is valid for
        // the full duration because the Arc is kept alive by `counter` in this
        // scope and both threads are joined before this scope exits.
        let f: fn(*mut i32) = unsafe { std::mem::transmute(ptr) };

        let t1 = {
            let p = ctr_ptr;
            std::thread::spawn(move || {
                for _ in 0..N {
                    f(p);
                }
            })
        };
        let t2 = {
            let p = ctr_ptr;
            std::thread::spawn(move || {
                for _ in 0..N {
                    f(p);
                }
            })
        };

        t1.join().expect("t1 panicked");
        t2.join().expect("t2 panicked");

        let final_count = counter.load(Ordering::SeqCst);
        assert_eq!(
            final_count,
            2 * N,
            "atomic counter: expected {} increments, got {}",
            2 * N,
            final_count,
        );
    }

    /// Four threads × 1000 iterations = 4000 — stress test for atomicity.
    ///
    /// A higher iteration count and more threads increase the window for data
    /// races; any lost update will cause the final assertion to fail.
    #[test]
    fn multithreaded_atomic_counter_large_n() {
        let jit = build_jit();

        let ptr = jit.get_function_ptr("increment").expect("symbol not found");

        let counter = Arc::new(AtomicI32::new(0));
        let ctr_ptr = counter.as_ref() as *const AtomicI32 as *mut i32;

        // SAFETY: same invariants as in `multithreaded_atomic_counter_no_lost_updates`.
        // `jit` outlives all spawned threads (they are joined below before this
        // function returns). `counter` keeps `ctr_ptr` valid for the full duration.
        let f: fn(*mut i32) = unsafe { std::mem::transmute(ptr) };

        let threads: Vec<_> = (0..4)
            .map(|_| {
                let p = ctr_ptr;
                std::thread::spawn(move || {
                    for _ in 0..1000 {
                        f(p);
                    }
                })
            })
            .collect();

        for t in threads {
            t.join().expect("thread panicked");
        }

        assert_eq!(counter.load(Ordering::SeqCst), 4000);
    }
}
