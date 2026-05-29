//! Tests for ThinLTO bitcode section embedding.

use llvm_bitcode::read_bitcode;
use llvm_codegen::emit::{ObjectFile, ObjectFormat};
use llvm_codegen::thinlto::embed_bitcode;
use llvm_ir::{Builder, Context, Linkage, Module};

// ── helpers ────────────────────────────────────────────────────────────────

/// Construct a minimal empty ObjectFile in ELF format.
fn elf_obj() -> ObjectFile {
    ObjectFile {
        format: ObjectFormat::Elf,
        elf_machine: 62, // EM_X86_64
        coff_machine: 0,
        sections: Vec::new(),
        symbols: Vec::new(),
    }
}

/// Construct a minimal empty ObjectFile in Mach-O format.
fn macho_obj() -> ObjectFile {
    ObjectFile {
        format: ObjectFormat::MachO,
        elf_machine: 0,
        coff_machine: 0,
        sections: Vec::new(),
        symbols: Vec::new(),
    }
}

/// Construct a minimal empty ObjectFile in COFF format.
fn coff_obj() -> ObjectFile {
    ObjectFile {
        format: ObjectFormat::Coff,
        elf_machine: 0,
        coff_machine: 0x8664, // x86-64
        sections: Vec::new(),
        symbols: Vec::new(),
    }
}

/// Build a trivial IR module with one function.
fn make_module(name: &str) -> (Context, Module) {
    let mut ctx = Context::new();
    let mut module = Module::new(name);
    let mut b = Builder::new(&mut ctx, &mut module);
    let i32_ty = b.ctx.i32_ty;
    b.add_function("f", i32_ty, vec![], vec![], false, Linkage::External);
    let entry = b.add_block("entry");
    b.position_at_end(entry);
    let c = b.const_int(i32_ty, 1);
    b.build_ret(c);
    (ctx, module)
}

// ── ELF tests ──────────────────────────────────────────────────────────────

#[test]
fn embed_bitcode_adds_llvmbc_section() {
    let (ctx, module) = make_module("test");
    let mut obj = elf_obj();
    embed_bitcode(&mut obj, &ctx, &module, "--opt-level=2").unwrap();
    let has_bc = obj.sections.iter().any(|s| s.name == ".llvmbc");
    assert!(has_bc, "expected .llvmbc section in ELF object");
}

#[test]
fn embed_bitcode_adds_llvmcmd_section() {
    let (ctx, module) = make_module("test");
    let mut obj = elf_obj();
    embed_bitcode(&mut obj, &ctx, &module, "--opt-level=2").unwrap();
    let cmd = obj.sections.iter().find(|s| s.name == ".llvmcmd").unwrap();
    assert_eq!(
        cmd.data, b"--opt-level=2",
        ".llvmcmd must contain the cmdline string"
    );
}

#[test]
fn embed_bitcode_bytes_nonempty() {
    let (ctx, module) = make_module("test");
    let mut obj = elf_obj();
    embed_bitcode(&mut obj, &ctx, &module, "").unwrap();
    let bc = obj.sections.iter().find(|s| s.name == ".llvmbc").unwrap();
    assert!(!bc.data.is_empty(), ".llvmbc section must not be empty");
}

#[test]
fn embed_bitcode_empty_cmdline() {
    let (ctx, module) = make_module("test");
    let mut obj = elf_obj();
    embed_bitcode(&mut obj, &ctx, &module, "").unwrap();
    let cmd = obj.sections.iter().find(|s| s.name == ".llvmcmd").unwrap();
    assert_eq!(
        cmd.data, b"",
        ".llvmcmd with empty cmdline must be zero bytes"
    );
}

#[test]
fn embed_bitcode_preserves_existing_sections() {
    use llvm_codegen::emit::Section;
    let (ctx, module) = make_module("test");
    let mut obj = elf_obj();
    // Pre-populate with a .text section.
    obj.sections.push(Section {
        name: ".text".into(),
        data: vec![0x90], // NOP
        relocs: Vec::new(),
        debug_rows: Vec::new(),
    });
    embed_bitcode(&mut obj, &ctx, &module, "").unwrap();
    assert_eq!(
        obj.sections[0].name, ".text",
        ".text must remain at index 0"
    );
    assert_eq!(
        obj.sections.len(),
        3,
        "must have .text + .llvmbc + .llvmcmd"
    );
}

// ── Mach-O tests ────────────────────────────────────────────────────────────

#[test]
fn embed_bitcode_macho_uses_llvm_segments() {
    let (ctx, module) = make_module("test");
    let mut obj = macho_obj();
    embed_bitcode(&mut obj, &ctx, &module, "-O2").unwrap();
    let has_bc = obj.sections.iter().any(|s| s.name == "__LLVM,__bitcode");
    let has_cmd = obj.sections.iter().any(|s| s.name == "__LLVM,__cmdline");
    assert!(has_bc, "Mach-O: expected __LLVM,__bitcode section");
    assert!(has_cmd, "Mach-O: expected __LLVM,__cmdline section");
}

#[test]
fn embed_bitcode_macho_no_elf_sections() {
    let (ctx, module) = make_module("test");
    let mut obj = macho_obj();
    embed_bitcode(&mut obj, &ctx, &module, "").unwrap();
    // Must NOT contain ELF-style names.
    let has_elf_bc = obj.sections.iter().any(|s| s.name == ".llvmbc");
    let has_elf_cmd = obj.sections.iter().any(|s| s.name == ".llvmcmd");
    assert!(!has_elf_bc, "Mach-O must not have .llvmbc section");
    assert!(!has_elf_cmd, "Mach-O must not have .llvmcmd section");
}

// ── COFF tests ────────────────────────────────────────────────────────────

#[test]
fn embed_bitcode_coff_uses_dot_llvmbc() {
    let (ctx, module) = make_module("test");
    let mut obj = coff_obj();
    embed_bitcode(&mut obj, &ctx, &module, "/O2").unwrap();
    let has_bc = obj.sections.iter().any(|s| s.name == ".llvmbc");
    let has_cmd = obj.sections.iter().any(|s| s.name == ".llvmcmd");
    assert!(has_bc, "COFF: expected .llvmbc section");
    assert!(has_cmd, "COFF: expected .llvmcmd section");
}

// ── round-trip test ────────────────────────────────────────────────────────

#[test]
fn thinlto_roundtrip_module_name() {
    // Embed the bitcode of "mymod", then parse the embedded bytes back using
    // the LRIR reader and verify the module name survives.
    let (ctx, module) = make_module("mymod");
    let mut obj = elf_obj();
    embed_bitcode(&mut obj, &ctx, &module, "").unwrap();

    let bc_sec = obj.sections.iter().find(|s| s.name == ".llvmbc").unwrap();
    let (_, module2) = read_bitcode(&bc_sec.data).expect("embedded bitcode must be valid LRIR");
    assert_eq!(
        module2.name, "mymod",
        "module name must survive the embed/read round-trip"
    );
}

#[test]
fn thinlto_roundtrip_function_count() {
    // A module with two functions: both must survive the round-trip.
    let mut ctx = Context::new();
    let mut module = Module::new("two_fn");
    let mut b = Builder::new(&mut ctx, &mut module);
    let i32_ty = b.ctx.i32_ty;

    b.add_function("foo", i32_ty, vec![], vec![], false, Linkage::External);
    let e1 = b.add_block("entry");
    b.position_at_end(e1);
    let c1 = b.const_int(i32_ty, 1);
    b.build_ret(c1);

    b.add_function("bar", i32_ty, vec![], vec![], false, Linkage::External);
    let e2 = b.add_block("entry");
    b.position_at_end(e2);
    let c2 = b.const_int(i32_ty, 2);
    b.build_ret(c2);
    drop(b);

    let mut obj = elf_obj();
    embed_bitcode(&mut obj, &ctx, &module, "").unwrap();

    let bc_sec = obj.sections.iter().find(|s| s.name == ".llvmbc").unwrap();
    let (_, module2) = read_bitcode(&bc_sec.data).expect("must parse");
    assert_eq!(
        module2.functions.len(),
        2,
        "both functions must survive ThinLTO round-trip"
    );
}
