//! Integration tests for the public (non-feature-gated) codegen backend API.

use llvm_rustc_backend::codegen_backend::codegen_backend_entrypoint;

#[test]
fn entrypoint_returns_version_string() {
    let s = codegen_backend_entrypoint();
    assert!(
        s.contains("llvm-in-rust"),
        "entrypoint string must mention 'llvm-in-rust', got: {s:?}"
    );
}

#[test]
fn backend_name_constant_is_stable() {
    assert_eq!(llvm_rustc_backend::BACKEND_NAME, "llvm-in-rust");
}
