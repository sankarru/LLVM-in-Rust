//! Integration tests for the standalone codegen driver.

use llvm_ir::{Builder, Context, Linkage, Module};
use llvm_rustc_backend::driver::{codegen_module, CodegenOptions, TargetArch};

// ── helpers ───────────────────────────────────────────────────────────────────

/// Build `define i32 @ret42() { entry: ret i32 42 }` programmatically.
fn make_ret42() -> (Context, Module) {
    let mut ctx = Context::new();
    let mut module = Module::new("test");
    let mut b = Builder::new(&mut ctx, &mut module);
    let i32_ty = b.ctx.i32_ty;
    b.add_function("ret42", i32_ty, vec![], vec![], false, Linkage::External);
    let entry = b.add_block("entry");
    b.position_at_end(entry);
    let c42 = b.const_int(i32_ty, 42);
    b.build_ret(c42);
    (ctx, module)
}

/// Build `define i32 @add(i32 %a, i32 %b) { entry: %r = add i32 %a, %b  ret i32 %r }`.
fn make_add_fn() -> (Context, Module) {
    let mut ctx = Context::new();
    let mut module = Module::new("test");
    let mut b = Builder::new(&mut ctx, &mut module);
    let i32_ty = b.ctx.i32_ty;
    b.add_function(
        "add",
        i32_ty,
        vec![i32_ty, i32_ty],
        vec!["a".into(), "b".into()],
        false,
        Linkage::External,
    );
    let entry = b.add_block("entry");
    b.position_at_end(entry);
    let a = b.get_arg(0);
    let bv = b.get_arg(1);
    let r = b.build_add("r", a, bv);
    b.build_ret(r);
    (ctx, module)
}

/// Build a module with only a declaration (no body).
fn make_decl_only() -> (Context, Module) {
    let mut ctx = Context::new();
    let mut module = Module::new("test");
    let void_ty = ctx.void_ty;
    let ptr_ty = ctx.ptr_ty;
    let mut b = Builder::new(&mut ctx, &mut module);
    b.add_declaration("printf", void_ty, vec![ptr_ty], true);
    let _ = b;
    (ctx, module)
}

// ── x86-64 tests ──────────────────────────────────────────────────────────────

#[test]
fn driver_emits_nonzero_bytes_for_trivial_function() {
    let (mut ctx, mut module) = make_ret42();
    let opts = CodegenOptions {
        target: TargetArch::X86_64,
        opt_level: 0,
    };
    let bytes = codegen_module(&mut ctx, &mut module, &opts)
        .expect("codegen must succeed for trivial function");
    assert!(!bytes.is_empty(), "output must be non-empty");
}

#[test]
fn driver_x86_elf_magic() {
    let (mut ctx, mut module) = make_ret42();
    let opts = CodegenOptions {
        target: TargetArch::X86_64,
        opt_level: 0,
    };
    let bytes = codegen_module(&mut ctx, &mut module, &opts).expect("must compile");
    assert_eq!(&bytes[..4], b"\x7fELF", "x86-64 output must be an ELF file");
}

#[test]
fn driver_x86_add_o0_compiles() {
    let (mut ctx, mut module) = make_add_fn();
    let opts = CodegenOptions {
        target: TargetArch::X86_64,
        opt_level: 0,
    };
    let bytes = codegen_module(&mut ctx, &mut module, &opts).expect("add O0 must compile");
    assert!(!bytes.is_empty());
}

#[test]
fn driver_x86_add_o2_compiles() {
    let (mut ctx, mut module) = make_add_fn();
    let opts = CodegenOptions {
        target: TargetArch::X86_64,
        opt_level: 2,
    };
    let bytes = codegen_module(&mut ctx, &mut module, &opts).expect("add O2 must compile");
    assert!(!bytes.is_empty());
}

#[test]
fn driver_x86_o3_pipeline_runs() {
    let (mut ctx, mut module) = make_add_fn();
    let opts = CodegenOptions {
        target: TargetArch::X86_64,
        opt_level: 3,
    };
    let bytes = codegen_module(&mut ctx, &mut module, &opts).expect("O3 must compile");
    assert!(!bytes.is_empty());
}

#[test]
fn driver_declarations_only_returns_error() {
    let (mut ctx, mut module) = make_decl_only();
    let opts = CodegenOptions {
        target: TargetArch::X86_64,
        opt_level: 0,
    };
    let result = codegen_module(&mut ctx, &mut module, &opts);
    assert!(
        result.is_err(),
        "declarations-only module must return an error"
    );
}

// ── AArch64 tests ─────────────────────────────────────────────────────────────

#[test]
fn driver_aarch64_emits_nonzero_bytes() {
    let (mut ctx, mut module) = make_ret42();
    let opts = CodegenOptions {
        target: TargetArch::AArch64,
        opt_level: 0,
    };
    let bytes = codegen_module(&mut ctx, &mut module, &opts).expect("AArch64 codegen must succeed");
    assert!(!bytes.is_empty(), "AArch64 output must be non-empty");
}

#[test]
fn driver_aarch64_elf_magic() {
    let (mut ctx, mut module) = make_ret42();
    let opts = CodegenOptions {
        target: TargetArch::AArch64,
        opt_level: 0,
    };
    let bytes = codegen_module(&mut ctx, &mut module, &opts).expect("must compile");
    assert_eq!(
        &bytes[..4],
        b"\x7fELF",
        "AArch64 ELF output must have ELF magic"
    );
}

#[test]
fn driver_aarch64_add_compiles() {
    let (mut ctx, mut module) = make_add_fn();
    let opts = CodegenOptions {
        target: TargetArch::AArch64,
        opt_level: 1,
    };
    let bytes = codegen_module(&mut ctx, &mut module, &opts).expect("AArch64 add must compile");
    assert!(!bytes.is_empty());
}

// ── codegen_backend entrypoint smoke test ─────────────────────────────────────

#[test]
fn backend_pipeline_smoke_via_driver() {
    // Exercises the full pipeline: IR construction → optimizer → isel → emit.
    let mut ctx = Context::new();
    let mut module = Module::new("smoke");
    let mut b = Builder::new(&mut ctx, &mut module);
    let i32_ty = b.ctx.i32_ty;
    b.add_function(
        "square",
        i32_ty,
        vec![i32_ty],
        vec!["x".into()],
        false,
        Linkage::External,
    );
    let entry = b.add_block("entry");
    b.position_at_end(entry);
    let x = b.get_arg(0);
    let r = b.build_mul("r", x, x);
    b.build_ret(r);
    let _ = b;

    let opts = CodegenOptions {
        target: TargetArch::X86_64,
        opt_level: 2,
    };
    let bytes = codegen_module(&mut ctx, &mut module, &opts).expect("smoke codegen must succeed");
    assert!(!bytes.is_empty(), "smoke test must emit non-empty object");
    // Symbol name must appear in the ELF string table.
    let has_sym = bytes
        .windows(b"\x00square\x00".len())
        .any(|w| w == b"\x00square\x00");
    assert!(has_sym, "symbol 'square' must appear in ELF strtab");
}
