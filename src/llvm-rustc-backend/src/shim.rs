//! Stable-compatible shim tests and helpers.
//!
//! These tests exercise the same pipeline stages a real rustc codegen backend
//! would invoke — parse IR, run optimisations, emit an object file — without
//! linking against `rustc_private` crates.  They serve as a contract: if the
//! shims pass, the backend pipeline is correct enough to wire up to rustc once
//! the nightly interface is available.

use llvm_codegen::{emit_object, IselBackend, ObjectFile, ObjectFormat, Reloc, Section, Symbol};
use llvm_ir::{Context, Module};
use llvm_ir_parser::parser::parse;
use llvm_target_x86::{TargetFeatures, X86Backend, X86Emitter};
use llvm_transforms::{build_pipeline, OptLevel};

/// Compile a snippet of LLVM IR text to a raw ELF object file.
///
/// This is the minimal pipeline a codegen backend must perform per
/// Compilation Unit (CGU):
/// 1. Parse IR text → `(Context, Module)`
/// 2. Run the optimisation pipeline
/// 3. Select instructions, allocate registers, emit machine code
pub fn compile_to_elf(ir: &str, opt: OptLevel) -> Result<Vec<u8>, String> {
    let (mut ctx, mut module) = parse(ir).map_err(|e| format!("parse: {e}"))?;

    let mut pm = build_pipeline(opt);
    pm.run_until_fixed_point(&mut ctx, &mut module, 8);

    emit_module_to_bytes(&ctx, &module, ObjectFormat::Elf)
}

/// Merge all non-declaration functions from `module` into a single x86-64
/// [`ObjectFile`] and serialise it.
///
/// **Target**: always x86-64 (uses [`X86Backend`] and [`X86Emitter`] internally).
/// The `fmt` parameter controls only the object *container* format (ELF / Mach-O /
/// COFF), not the instruction set.  When relocs span multiple per-function objects,
/// symbol indices are remapped so the merged file is self-consistent.
pub fn emit_module_to_bytes(
    ctx: &Context,
    module: &Module,
    fmt: ObjectFormat,
) -> Result<Vec<u8>, String> {
    let text_name = if fmt == ObjectFormat::MachO {
        "__text"
    } else {
        ".text"
    };

    let mut merged_text: Vec<u8> = Vec::new();
    let mut merged_symbols: Vec<Symbol> = Vec::new();
    let mut merged_relocs: Vec<Reloc> = Vec::new();

    for func in module.functions.iter() {
        if func.is_declaration {
            continue;
        }

        let mut backend = X86Backend::new(TargetFeatures::baseline());
        let mf = backend.lower_function(ctx, module, func);

        let mut emitter = X86Emitter::new(fmt);
        let obj = emit_object(&mf, &mut emitter);

        let text_off = merged_text.len();
        let sym_base = merged_symbols.len();

        // Find the text section and accumulate.
        if let Some(sec) = obj
            .sections
            .iter()
            .find(|s| s.name == ".text" || s.name == "__text")
        {
            for mut r in sec.relocs.iter().cloned() {
                r.symbol += sym_base;
                r.offset += text_off as u64;
                merged_relocs.push(r);
            }
            merged_text.extend_from_slice(&sec.data);
        }

        for mut sym in obj.symbols {
            if !sym.undefined {
                sym.offset += text_off as u64;
            }
            merged_symbols.push(sym);
        }
    }

    if merged_text.is_empty() {
        return Err("no code emitted — module has no non-declaration functions".into());
    }

    let merged = ObjectFile {
        format: fmt,
        elf_machine: if fmt == ObjectFormat::Elf { 62 } else { 0 },
        coff_machine: if fmt == ObjectFormat::Coff { 0x8664 } else { 0 },
        sections: vec![Section {
            name: text_name.into(),
            data: merged_text,
            relocs: merged_relocs,
            debug_rows: vec![],
        }],
        symbols: merged_symbols,
    };

    Ok(merged.to_bytes())
}

// ── shim tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    const RETURN_CONST: &str = "define i32 @main() { entry: ret i32 0 }";

    const SIMPLE_ADD: &str =
        "define i32 @add(i32 %a, i32 %b) { entry: %r = add i32 %a, %b  ret i32 %r }";

    const MULTI_FUNC: &str = "
        define i32 @square(i32 %x) { entry: %r = mul i32 %x, %x  ret i32 %r }
        define i32 @main() {
          entry:
            %v = call i32 @square(i32 5)
            ret i32 %v
        }
    ";

    const CONDITIONAL: &str = "
        define i32 @abs_val(i32 %x) {
          entry:
            %neg = icmp slt i32 %x, 0
            br i1 %neg, label %neg_block, label %pos_block
          neg_block:
            %n = sub i32 0, %x
            ret i32 %n
          pos_block:
            ret i32 %x
        }
    ";

    const RECURSIVE: &str = "
        define i32 @fib(i32 %n) {
          entry:
            %cmp = icmp slt i32 %n, 2
            br i1 %cmp, label %base, label %rec
          base:
            ret i32 %n
          rec:
            %nm1 = sub i32 %n, 1
            %nm2 = sub i32 %n, 2
            %r1 = call i32 @fib(i32 %nm1)
            %r2 = call i32 @fib(i32 %nm2)
            %sum = add i32 %r1, %r2
            ret i32 %sum
        }
    ";

    // Per-CGU pipeline correctness

    #[test]
    fn shim_single_cgu_return_const() {
        compile_to_elf(RETURN_CONST, OptLevel::O0).expect("single-CGU compile must succeed");
    }

    #[test]
    fn shim_single_cgu_add_o0() {
        let bytes = compile_to_elf(SIMPLE_ADD, OptLevel::O0).expect("add O0 must compile");
        assert!(!bytes.is_empty(), "object must be non-empty");
    }

    #[test]
    fn shim_single_cgu_add_o2() {
        let bytes = compile_to_elf(SIMPLE_ADD, OptLevel::O2).expect("add O2 must compile");
        assert!(!bytes.is_empty(), "object must be non-empty");
    }

    #[test]
    fn shim_multi_func_cgu_emits_both_symbols() {
        let bytes = compile_to_elf(MULTI_FUNC, OptLevel::O0).expect("multi-func must compile");
        // ELF strtab stores null-terminated symbol names; match the null bytes on
        // both sides to avoid false positives from section headers or other data.
        let has = |name: &[u8]| {
            bytes
                .windows(name.len() + 2)
                .any(|w| w[0] == 0 && &w[1..name.len() + 1] == name && w[name.len() + 1] == 0)
        };
        assert!(has(b"square"), "symbol 'square' must appear in strtab");
        assert!(has(b"main"), "symbol 'main' must appear in strtab");
    }

    #[test]
    fn shim_conditional_branch_compiles() {
        compile_to_elf(CONDITIONAL, OptLevel::O1).expect("conditional branch must compile");
    }

    #[test]
    fn shim_recursive_function_compiles() {
        compile_to_elf(RECURSIVE, OptLevel::O0).expect("recursive function must compile");
    }

    #[test]
    fn shim_o2_optimisation_pipeline_runs() {
        compile_to_elf(MULTI_FUNC, OptLevel::O2).expect("O2 pipeline must not fail");
    }

    #[test]
    fn shim_elf_magic_bytes() {
        let bytes = compile_to_elf(RETURN_CONST, OptLevel::O0).expect("must compile");
        assert_eq!(&bytes[..4], b"\x7fELF", "ELF magic must be present");
    }

    #[test]
    fn shim_declarations_only_returns_error() {
        let ir = "declare i32 @printf(i8*, ...)";
        let result = compile_to_elf(ir, OptLevel::O0);
        assert!(
            result.is_err(),
            "declarations-only module must return an error"
        );
    }

    // CGU idempotency: running the pipeline twice must not change function count.
    #[test]
    fn shim_pipeline_is_idempotent_at_o1() {
        let (mut ctx, mut module) = parse(SIMPLE_ADD).expect("parse");
        let mut pm = build_pipeline(OptLevel::O1);
        pm.run_until_fixed_point(&mut ctx, &mut module, 8);
        let func_count_after_first = module.num_functions();
        let mut pm2 = build_pipeline(OptLevel::O1);
        pm2.run_until_fixed_point(&mut ctx, &mut module, 8);
        assert_eq!(
            module.num_functions(),
            func_count_after_first,
            "function count must be stable after second pass"
        );
    }

    // Format coverage: verify the container format branching in emit_module_to_bytes.
    #[cfg(target_os = "macos")]
    #[test]
    fn shim_macho_magic_bytes() {
        let (mut ctx, mut module) = parse(RETURN_CONST).expect("parse");
        let mut pm = build_pipeline(OptLevel::O0);
        pm.run_until_fixed_point(&mut ctx, &mut module, 8);
        let bytes = emit_module_to_bytes(&ctx, &module, ObjectFormat::MachO)
            .expect("Mach-O emit must succeed");
        // Mach-O magic: 0xCFFAEDFE (little-endian 64-bit)
        assert_eq!(
            &bytes[..4],
            b"\xcf\xfa\xed\xfe",
            "Mach-O magic must be present"
        );
    }

    #[test]
    fn shim_coff_magic_bytes() {
        let (mut ctx, mut module) = parse(RETURN_CONST).expect("parse");
        let mut pm = build_pipeline(OptLevel::O0);
        pm.run_until_fixed_point(&mut ctx, &mut module, 8);
        let bytes = emit_module_to_bytes(&ctx, &module, ObjectFormat::Coff)
            .expect("COFF emit must succeed");
        // COFF x86-64 machine type: 0x8664 (little-endian)
        assert_eq!(
            &bytes[..2],
            b"\x64\x86",
            "COFF x86-64 magic must be present"
        );
    }

    #[test]
    fn shim_backend_name_is_stable() {
        assert_eq!(crate::BACKEND_NAME, "llvm-in-rust");
    }
}
