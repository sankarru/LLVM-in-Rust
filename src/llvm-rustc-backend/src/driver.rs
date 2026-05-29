//! Standalone codegen driver — does NOT require rustc internals.
//!
//! Given an LLVM IR module, runs the full optimize → instruction-select →
//! register-allocate → emit pipeline and returns raw object-file bytes.
//!
//! This is the same pipeline the shim tests in `shim.rs` use, but exposed as
//! a public API with explicit `CodegenOptions` so callers can control the
//! target architecture and optimisation level.
//!
//! # Example
//!
//! ```no_run
//! use llvm_ir::{Context, Module};
//! use llvm_rustc_backend::driver::{CodegenOptions, TargetArch, codegen_module};
//! let mut ctx = Context::new();
//! let mut module = Module::new("hello");
//! // ... populate the module with IR ...
//! let opts = CodegenOptions { target: TargetArch::X86_64, opt_level: 0 };
//! let bytes = codegen_module(&mut ctx, &mut module, &opts).expect("codegen failed");
//! assert!(!bytes.is_empty());
//! ```

use llvm_codegen::{emit_object, IselBackend, ObjectFile, ObjectFormat, Reloc, Section, Symbol};
use llvm_ir::{Context, Module};
use llvm_target_arm::encode::AArch64Emitter;
use llvm_target_arm::lower::{AArch64Backend, AArch64Features};
use llvm_target_x86::{TargetFeatures, X86Backend, X86Emitter};
use llvm_transforms::{build_pipeline, OptLevel};

/// Target architecture for code generation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TargetArch {
    /// 64-bit x86 (AMD64 / Intel 64).
    X86_64,
    /// 64-bit ARM (AArch64 / ARM64).
    AArch64,
}

/// Options controlling the codegen pipeline.
#[derive(Clone, Debug)]
pub struct CodegenOptions {
    /// Target instruction set architecture.
    pub target: TargetArch,
    /// Optimization level: 0 = none, 1 = basic, 2 = standard, 3 = aggressive.
    pub opt_level: u32,
}

impl Default for CodegenOptions {
    fn default() -> Self {
        Self {
            target: TargetArch::X86_64,
            opt_level: 0,
        }
    }
}

impl CodegenOptions {
    /// Convert the numeric opt_level to the pipeline enum.
    fn pipeline_level(&self) -> OptLevel {
        match self.opt_level {
            0 => OptLevel::O0,
            1 => OptLevel::O1,
            2 => OptLevel::O2,
            _ => OptLevel::O3,
        }
    }

    /// Object format derived from the host platform and target.
    fn object_format(&self) -> ObjectFormat {
        // Use ELF for all architectures; callers that need Mach-O on macOS can
        // call `emit_module_to_bytes` directly via `shim.rs`.
        ObjectFormat::Elf
    }
}

/// Compile an IR module to a raw ELF object file.
///
/// Steps:
/// 1. Run the optimisation pipeline at the requested level.
/// 2. Lower each non-declaration function through the target backend.
/// 3. Merge the per-function `ObjectFile`s into a single file.
/// 4. Serialise and return the raw bytes.
///
/// Returns `Err` if the module contains no non-declaration functions or if
/// any individual function fails to emit.
pub fn codegen_module(
    ctx: &mut Context,
    module: &mut Module,
    opts: &CodegenOptions,
) -> Result<Vec<u8>, String> {
    // 1. Optimise.
    let mut pm = build_pipeline(opts.pipeline_level());
    pm.run_until_fixed_point(ctx, module, 8);

    let fmt = opts.object_format();
    let text_name = if fmt == ObjectFormat::MachO {
        "__text"
    } else {
        ".text"
    };

    // 2 + 3. Lower each function and merge.
    let mut merged_text: Vec<u8> = Vec::new();
    let mut merged_symbols: Vec<Symbol> = Vec::new();
    let mut merged_relocs: Vec<Reloc> = Vec::new();
    let mut any_code = false;

    for func in module.functions.iter() {
        if func.is_declaration {
            continue;
        }

        let obj: ObjectFile = match opts.target {
            TargetArch::X86_64 => {
                let mut backend = X86Backend::new(TargetFeatures::baseline());
                let mf = backend.lower_function(ctx, module, func);
                let mut emitter = X86Emitter::new(fmt);
                emit_object(&mf, &mut emitter)
            }
            TargetArch::AArch64 => {
                let aarch64_fmt = match fmt {
                    ObjectFormat::MachO => ObjectFormat::MachO,
                    ObjectFormat::Coff => ObjectFormat::Coff,
                    ObjectFormat::Elf => ObjectFormat::Elf,
                };
                let mut backend = AArch64Backend::new(AArch64Features::lse());
                let mf = backend.lower_function(ctx, module, func);
                let mut emitter = AArch64Emitter::new(aarch64_fmt);
                emit_object(&mf, &mut emitter)
            }
        };

        let text_off = merged_text.len();
        let sym_base = merged_symbols.len();

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
            any_code = true;
        }

        for mut sym in obj.symbols {
            if !sym.undefined {
                sym.offset += text_off as u64;
            }
            merged_symbols.push(sym);
        }
    }

    if !any_code {
        return Err("codegen_module: module has no non-declaration functions".into());
    }

    // 4. Build merged ObjectFile and serialise.
    let elf_machine: u16 = match opts.target {
        TargetArch::X86_64 => 62,   // EM_X86_64
        TargetArch::AArch64 => 183, // EM_AARCH64
    };

    let merged = ObjectFile {
        format: fmt,
        elf_machine,
        coff_machine: 0,
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
