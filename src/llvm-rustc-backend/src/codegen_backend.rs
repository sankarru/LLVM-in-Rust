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
//! All methods are `todo!()` stubs.  The MIR→IR translation layer (the largest
//! gap) must be implemented before any of these methods can do real work.  See
//! `docs/rustc-backend-gap-analysis.md` for a full gap analysis.

// Nightly feature required to access rustc internals.
#![allow(unused_variables, dead_code)]

// When this module is compiled, the caller is responsible for ensuring
// `#![feature(rustc_private)]` is active in the crate root.

/// Entry point symbol that rustc's dynamic loader calls when the backend dylib
/// is loaded via `-Zcodegen-backend=<path>`.
///
/// # Safety
///
/// Called by rustc's plugin loader; must be `unsafe extern "C"`.
#[no_mangle]
pub unsafe extern "C" fn __rustc_codegen_backend() -> *mut () {
    // TODO: return a Box<dyn CodegenBackend> cast to *mut ().
    // Requires extern crate rustc_codegen_ssa once rustc_private is active.
    todo!("rustc codegen backend entry point not yet implemented")
}

// TODO items (in implementation order):
//
// 1. Add `#![feature(rustc_private)]` to lib.rs (only when this feature is on).
// 2. `extern crate rustc_codegen_ssa;`
//    `extern crate rustc_middle;`
//    `extern crate rustc_target;`
// 3. Define `struct LlvmInRustBackend;`
// 4. Implement `CodegenBackend for LlvmInRustBackend`:
//    - `init(&self, sess: &Session)`
//    - `print(&self, req: &PrintRequest, sess: &Session)`
//    - `codegen_crate(&self, tcx: TyCtxt, metadata, …) -> Box<dyn Any>`
//    - `join_codegen(&self, ongoing, sess, outputs) -> (CodegenResults, FxHashMap<…>)`
//    - `link(&self, sess, codegen_results, outputs)`
// 5. Map rustc CGU → our `Module` in `codegen_crate`.
// 6. For each CGU: lower MIR BasicBlocks → llvm-ir BasicBlocks.
// 7. Run `build_pipeline(opt_level)` on the resulting Module.
// 8. Call `emit_module_to_bytes` → pass ObjectFile bytes to `join_codegen`.
