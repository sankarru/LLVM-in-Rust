//! Nightly-only `rustc_codegen_ssa::traits::CodegenBackend` implementation.
//!
//! This module is compiled only when `--features rustc-backend` is active.
//! It requires a nightly toolchain with the `rustc-dev` component installed.
//!
//! # How to build
//!
//! ```sh
//! rustup override set nightly
//! rustup component add rustc-dev
//! cargo build --features rustc-backend
//! ```
//!
//! # Status
//!
//! All nightly-specific methods are gated behind `#[cfg(feature = "rustc-backend")]`.
//! The public entry point `codegen_backend_entrypoint` is always available.

#![allow(unused_variables, dead_code)]

// ── always-available public API ───────────────────────────────────────────────

/// Return the backend name/version string.
///
/// This does **not** require rustc internals — it can be used from stable
/// tests to verify the crate is wired up correctly.
pub fn codegen_backend_entrypoint() -> &'static str {
    "llvm-in-rust codegen backend v0.1"
}

// ── nightly-only rustc integration ────────────────────────────────────────────

#[cfg(feature = "rustc-backend")]
mod real_backend {
    //! Feature-gated implementation using `rustc_codegen_ssa` traits.
    //!
    //! This compiles only when both:
    //!  * `--features rustc-backend` is passed to Cargo, AND
    //!  * a nightly toolchain with `rustc-dev` is active.
    //!
    //! The `#![feature(rustc_private)]` attribute is required in the crate root
    //! to access `rustc_codegen_ssa` and related internal crates.  Add it to
    //! `lib.rs` when building with this feature.

    use llvm_codegen::isel::IselBackend;
    use llvm_target_arm::lower::{AArch64Backend, AArch64Features};
    use llvm_target_x86::{TargetFeatures, X86Backend};
    use std::sync::{Arc, Mutex};

    /// The LLVM-in-Rust codegen backend.
    ///
    /// One instance is created per compilation session.  The `target_machine`
    /// mutex holds the lazily-initialised backend, which is selected based on
    /// the target triple provided to `init`.
    pub struct LlvmInRustBackend {
        pub(crate) target_machine: Arc<Mutex<Option<Box<dyn IselBackend + Send>>>>,
    }

    impl LlvmInRustBackend {
        /// Create a new backend instance with no target machine yet selected.
        pub fn new() -> Self {
            Self {
                target_machine: Arc::new(Mutex::new(None)),
            }
        }

        /// Select the right instruction-selection backend for `triple`.
        ///
        /// Defaults to x86-64 for any unrecognised triple.
        pub fn make_backend(triple: &str) -> Box<dyn IselBackend + Send> {
            if triple.contains("aarch64") || triple.contains("arm64") {
                Box::new(AArch64Backend::new(AArch64Features::lse()))
            } else {
                Box::new(X86Backend::new(TargetFeatures::baseline()))
            }
        }
    }

    impl Default for LlvmInRustBackend {
        fn default() -> Self {
            Self::new()
        }
    }

    // TODO: when #![feature(rustc_private)] + rustc-dev are available, add:
    //
    //   extern crate rustc_codegen_ssa;
    //   extern crate rustc_middle;
    //   extern crate rustc_target;
    //
    // and implement `rustc_codegen_ssa::traits::CodegenBackend`:
    //
    //   impl CodegenBackend for LlvmInRustBackend {
    //       fn init(&self, sess: &Session) { ... }
    //       fn codegen_crate(&self, tcx: TyCtxt, ...) -> Box<dyn Any> { ... }
    //       fn join_codegen(...) -> (CodegenResults, FxHashMap<...>) { ... }
    //       fn link(&self, sess, codegen_results, outputs) { ... }
    //   }
    //
    // Each CGU in `codegen_crate` maps to a call to
    // `crate::driver::codegen_module(ctx, &mut module, &opts)`.
}

// ── __rustc_codegen_backend symbol ────────────────────────────────────────────
//
// rustc's dynamic-plugin loader dlsym()s for `__rustc_codegen_backend` when
// `-Zcodegen-backend=<path>` is passed.  This symbol is only meaningful in a
// `rustc-backend` build; on stable the function body is a no-op stub.

/// Entry point called by rustc when this dylib is loaded as a codegen backend.
///
/// # Safety
///
/// Called by rustc's plugin loader via `dlsym`; must be `unsafe extern "C"`.
///
/// Without `--features rustc-backend` this returns a null pointer and is
/// effectively unused.  With the feature enabled, it allocates and returns
/// a `LlvmInRustBackend` as a type-erased raw pointer for rustc to use.
#[no_mangle]
pub unsafe extern "C" fn __rustc_codegen_backend() -> *mut () {
    #[cfg(feature = "rustc-backend")]
    {
        let backend = real_backend::LlvmInRustBackend::new();
        Box::into_raw(Box::new(backend)) as *mut ()
    }
    #[cfg(not(feature = "rustc-backend"))]
    {
        // Stable build: symbol must exist for linking but is never called.
        std::ptr::null_mut()
    }
}
