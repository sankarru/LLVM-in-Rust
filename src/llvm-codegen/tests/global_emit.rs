//! Tests for [`emit_globals`]: byte layout, relocations, and ELF section
//! emission for global variables including `ConstantData::Expr` initializers.

use llvm_codegen::{emit_globals, sizeof_ty, ObjectFormat, RelocKind};
use llvm_ir::{Context, Linkage, Module};
use llvm_ir_parser::parser::parse;

fn elf() -> ObjectFormat {
    ObjectFormat::Elf
}

// ── sizeof_ty ────────────────────────────────────────────────────────────────

#[test]
fn sizeof_scalars() {
    let ctx = Context::new();
    assert_eq!(sizeof_ty(&ctx, ctx.i8_ty), 1);
    assert_eq!(sizeof_ty(&ctx, ctx.i16_ty), 2);
    assert_eq!(sizeof_ty(&ctx, ctx.i32_ty), 4);
    assert_eq!(sizeof_ty(&ctx, ctx.i64_ty), 8);
    assert_eq!(sizeof_ty(&ctx, ctx.ptr_ty), 8);
    assert_eq!(sizeof_ty(&ctx, ctx.f32_ty), 4);
    assert_eq!(sizeof_ty(&ctx, ctx.f64_ty), 8);
    assert_eq!(sizeof_ty(&ctx, ctx.void_ty), 0);
}

#[test]
fn sizeof_array() {
    let mut ctx = Context::new();
    let arr = ctx.mk_array(ctx.i32_ty, 4);
    assert_eq!(sizeof_ty(&ctx, arr), 16);
}

#[test]
fn sizeof_packed_struct() {
    let mut ctx = Context::new();
    let st = ctx.mk_struct_anon(vec![ctx.i8_ty, ctx.i32_ty], /*packed=*/ true);
    assert_eq!(sizeof_ty(&ctx, st), 5);
}

#[test]
fn sizeof_padded_struct() {
    let mut ctx = Context::new();
    // { i8, pad[3], i32 } → 8 bytes (natural alignment = 4)
    let st = ctx.mk_struct_anon(vec![ctx.i8_ty, ctx.i32_ty], /*packed=*/ false);
    assert_eq!(sizeof_ty(&ctx, st), 8);
}

// ── emit_globals: no globals ─────────────────────────────────────────────────

#[test]
fn emit_globals_empty_module_returns_none() {
    let ctx = Context::new();
    let module = Module::new("test".to_string());
    assert!(emit_globals(&ctx, &module, elf()).is_none());
}

// ── emit_globals: simple scalar global ───────────────────────────────────────

#[test]
fn emit_globals_simple_i32() {
    let (ctx, module) = parse(
        r#"
@x = global i32 42
"#,
    )
    .unwrap();
    let (secs, syms) = emit_globals(&ctx, &module, elf()).unwrap();
    // i32 global → .data section (mutable)
    assert_eq!(secs.len(), 1);
    assert_eq!(secs[0].name, ".data");
    // 4 bytes, little-endian 42
    assert_eq!(&secs[0].data[..4], &[42, 0, 0, 0]);
    assert!(secs[0].relocs.is_empty());
    assert_eq!(syms.len(), 1);
    assert_eq!(syms[0].name, "x");
    assert_eq!(syms[0].size, 4);
}

#[test]
fn emit_globals_constant_rodata() {
    let (ctx, module) = parse(
        r#"
@c = constant i64 0x0102030405060708
"#,
    )
    .unwrap();
    let (secs, _syms) = emit_globals(&ctx, &module, elf()).unwrap();
    assert_eq!(secs[0].name, ".rodata");
    assert_eq!(
        &secs[0].data[..8],
        &[0x08, 0x07, 0x06, 0x05, 0x04, 0x03, 0x02, 0x01]
    );
}

// ── emit_globals: array ───────────────────────────────────────────────────────

#[test]
fn emit_globals_i32_array() {
    let (ctx, module) = parse(
        r#"
@arr = constant [4 x i32] [i32 0, i32 1, i32 2, i32 3]
"#,
    )
    .unwrap();
    let (secs, syms) = emit_globals(&ctx, &module, elf()).unwrap();
    assert_eq!(secs[0].name, ".rodata");
    assert_eq!(secs[0].data.len(), 16);
    assert_eq!(&secs[0].data[..4], &[0, 0, 0, 0]);
    assert_eq!(&secs[0].data[4..8], &[1, 0, 0, 0]);
    assert_eq!(&secs[0].data[8..12], &[2, 0, 0, 0]);
    assert_eq!(&secs[0].data[12..16], &[3, 0, 0, 0]);
    assert_eq!(syms[0].size, 16);
}

// ── emit_globals: GlobalRef pointer ──────────────────────────────────────────

#[test]
fn emit_globals_globalref_emits_abs64_reloc() {
    // Construct the module programmatically because `@ptr = constant ptr @base`
    // is parsed as a typed GlobalRef only when the parser resolves the type.
    let mut ctx = Context::new();
    let mut module = Module::new("test".to_string());

    // @base = constant i32 99
    let base_init = ctx.const_int(ctx.i32_ty, 99);
    let base_gid = module.add_global(llvm_ir::GlobalVariable {
        name: "base".to_string(),
        ty: ctx.i32_ty,
        initializer: Some(base_init),
        is_constant: true,
        linkage: Linkage::External,
    });

    // @ptr = constant ptr @base  (GlobalRef)
    let ptr_init = ctx.push_const(llvm_ir::ConstantData::GlobalRef {
        ty: ctx.ptr_ty,
        id: base_gid,
        name: "base".to_string(),
    });
    module.add_global(llvm_ir::GlobalVariable {
        name: "ptr".to_string(),
        ty: ctx.ptr_ty,
        initializer: Some(ptr_init),
        is_constant: true,
        linkage: Linkage::External,
    });

    let (secs, syms) = emit_globals(&ctx, &module, elf()).unwrap();
    // Both are constant → single .rodata section.
    assert_eq!(secs.len(), 1, "expected single .rodata section");
    assert_eq!(secs[0].name, ".rodata");

    // @ptr occupies 8 bytes after @i32-aligned @base (4 bytes, padded to 8).
    let ptr_sym = syms.iter().find(|s| s.name == "ptr").unwrap();
    let ptr_off = ptr_sym.offset;

    // Find the Abs64 reloc for @ptr.
    let reloc = secs[0]
        .relocs
        .iter()
        .find(|r| r.offset == ptr_off)
        .expect("expected Abs64 reloc for @ptr");
    assert_eq!(reloc.kind, RelocKind::Abs64);
    assert_eq!(reloc.addend, 0);
    let base_sym_idx = syms.iter().position(|s| s.name == "base").unwrap();
    assert_eq!(reloc.symbol, base_sym_idx);
}

// ── emit_globals: GEP constexpr ──────────────────────────────────────────────

#[test]
fn emit_globals_gep_constexpr_reloc_with_addend() {
    let (ctx, module) = parse(
        r#"
@base = constant [4 x i32] [i32 0, i32 1, i32 2, i32 3]
@p3   = constant ptr getelementptr inbounds ([4 x i32], ptr @base, i64 0, i64 3)
"#,
    )
    .unwrap();
    let (secs, syms) = emit_globals(&ctx, &module, elf()).unwrap();
    assert_eq!(secs.len(), 1); // both constant → .rodata
    // @p3 → 8 zero bytes + reloc with addend = 3 * sizeof(i32) = 12
    let p3_sym = syms.iter().find(|s| s.name == "p3").unwrap();
    let reloc = secs[0]
        .relocs
        .iter()
        .find(|r| r.offset == p3_sym.offset)
        .expect("expected GEP reloc");
    assert_eq!(reloc.kind, RelocKind::Abs64);
    assert_eq!(reloc.addend, 12, "offset of element [3] in [4 x i32] = 12");
    let base_sym_idx = syms.iter().position(|s| s.name == "base").unwrap();
    assert_eq!(reloc.symbol, base_sym_idx);
}

// ── emit_globals: mixed const / mutable ──────────────────────────────────────

#[test]
fn emit_globals_mixed_const_and_mutable() {
    let (ctx, module) = parse(
        r#"
@c = constant i32 1
@v = global   i32 2
"#,
    )
    .unwrap();
    let (secs, syms) = emit_globals(&ctx, &module, elf()).unwrap();
    assert_eq!(secs.len(), 2);
    let names: Vec<&str> = secs.iter().map(|s| s.name.as_str()).collect();
    assert!(names.contains(&".rodata"), "expected .rodata");
    assert!(names.contains(&".data"), "expected .data");
    assert_eq!(syms.len(), 2);
}

// ── emit_globals: zero-initializer ───────────────────────────────────────────

#[test]
fn emit_globals_zeroinitializer() {
    let (ctx, module) = parse(
        r#"
@buf = global [8 x i8] zeroinitializer
"#,
    )
    .unwrap();
    let (secs, _syms) = emit_globals(&ctx, &module, elf()).unwrap();
    assert_eq!(secs[0].data, vec![0u8; 8]);
}

// ── ELF integration: rodata section appears in serialized output ─────────────

#[test]
fn emit_globals_elf_rodata_section_present() {
    let (ctx, module) = parse(
        r#"
@arr = constant [4 x i32] [i32 10, i32 20, i32 30, i32 40]
"#,
    )
    .unwrap();
    let (secs, syms) = emit_globals(&ctx, &module, elf()).unwrap();
    use llvm_codegen::ObjectFile;
    let obj = ObjectFile {
        format: elf(),
        elf_machine: 62,
        coff_machine: 0,
        sections: secs,
        symbols: syms,
    };
    let bytes = obj.to_bytes();
    // ELF magic present
    assert_eq!(&bytes[..4], b"\x7fELF");
    // ".rodata" name appears in the ELF output
    let needle = b".rodata";
    assert!(
        bytes.windows(needle.len()).any(|w| w == needle),
        ".rodata section name not found in ELF output"
    );
    // Content appears: [10, 0, 0, 0, 20, 0, 0, 0, ...]
    let data_needle = &[10u8, 0, 0, 0, 20, 0, 0, 0];
    assert!(
        bytes.windows(data_needle.len()).any(|w| w == data_needle),
        "global array data not found in ELF output"
    );
}

// ── ELF integration: GEP reloc in serialized output ──────────────────────────

#[test]
fn emit_globals_elf_gep_reloc_present() {
    let (ctx, module) = parse(
        r#"
@base = constant [4 x i32] [i32 0, i32 1, i32 2, i32 3]
@p3   = constant ptr getelementptr inbounds ([4 x i32], ptr @base, i64 0, i64 3)
"#,
    )
    .unwrap();
    let (secs, syms) = emit_globals(&ctx, &module, elf()).unwrap();
    use llvm_codegen::ObjectFile;
    let obj = ObjectFile {
        format: elf(),
        elf_machine: 62,
        coff_machine: 0,
        sections: secs,
        symbols: syms,
    };
    let bytes = obj.to_bytes();
    // .rela.rodata section name must appear
    let needle = b".rela.rodata";
    assert!(
        bytes.windows(needle.len()).any(|w| w == needle),
        ".rela.rodata section not found in ELF output"
    );
    // Addend of 12 (0x0C000000_00000000 in LE i64) must appear
    let addend_bytes: [u8; 8] = 12i64.to_le_bytes();
    assert!(
        bytes.windows(8).any(|w| w == addend_bytes),
        "addend 12 not found in ELF relocation table"
    );
}

// ── compile_ir_to_object integration ─────────────────────────────────────────

#[test]
fn compile_ir_with_globals_produces_rodata_section() {
    use llvm_codegen::ObjectFormat;
    use llvm_ir_parser::parser::parse;

    let src = r#"
@data = constant [4 x i32] [i32 1, i32 2, i32 3, i32 4]

define i32 @get_sum() {
entry:
  ret i32 10
}
"#;
    let (ctx, module) = parse(src).unwrap();
    let (secs, _syms) = emit_globals(&ctx, &module, ObjectFormat::Elf).unwrap();
    assert_eq!(secs[0].name, ".rodata");
    assert_eq!(secs[0].data.len(), 16);
}
