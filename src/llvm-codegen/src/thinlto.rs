//! ThinLTO bitcode section embedding.
//!
//! Linkers that support ThinLTO (lld, gold-plugin, Apple ld) expect the
//! module bitcode to be embedded in a dedicated object-file section:
//!
//! | Format        | Bitcode section        | Cmdline section         |
//! |---------------|------------------------|-------------------------|
//! | ELF / COFF    | `.llvmbc`              | `.llvmcmd`              |
//! | Mach-O        | `__LLVM,__bitcode`     | `__LLVM,__cmdline`      |
//!
//! This module serialises the IR to the LRIR binary format and appends
//! the two sections to an existing [`ObjectFile`].
//!
//! # Example
//!
//! ```no_run
//! use llvm_ir::{Context, Module};
//! use llvm_codegen::emit::{ObjectFile, ObjectFormat};
//! use llvm_codegen::thinlto::embed_bitcode;
//! let ctx = Context::new();
//! let module = Module::new("mymod");
//! let mut obj = ObjectFile { format: ObjectFormat::Elf, elf_machine: 62,
//!     coff_machine: 0, sections: vec![], symbols: vec![] };
//! embed_bitcode(&mut obj, &ctx, &module, "--opt-level=2").unwrap();
//! assert!(obj.sections.iter().any(|s| s.name == ".llvmbc"));
//! ```

use crate::emit::{ObjectFile, ObjectFormat, Section};
use llvm_bitcode::write_bitcode;
use llvm_ir::{Context, Module};

/// Add a `.llvmbc` / `__LLVM,__bitcode` section (and the matching cmdline
/// section) to `obj`, containing the LRIR-serialised bitcode of `module`.
///
/// The section names follow the ThinLTO convention used by LLVM's linker
/// plugin and lld:
/// * ELF / COFF: `.llvmbc` and `.llvmcmd`
/// * Mach-O: `__LLVM,__bitcode` and `__LLVM,__cmdline`
///
/// This function appends to `obj.sections` and never removes existing sections.
///
/// # Errors
///
/// Returns `Err` only if bitcode serialisation fails (currently infallible,
/// but the `Result` type future-proofs the API).
pub fn embed_bitcode(
    obj: &mut ObjectFile,
    ctx: &Context,
    module: &Module,
    cmdline: &str,
) -> Result<(), String> {
    // 1. Serialise module to LRIR bitcode.
    let bc_bytes = write_bitcode(ctx, module);

    if bc_bytes.is_empty() {
        return Err("embed_bitcode: write_bitcode produced empty output".into());
    }

    // 2. Determine section names based on object format.
    let (bc_name, cmd_name) = section_names(obj.format);

    // 3. Append .llvmbc / __LLVM,__bitcode.
    obj.sections.push(Section {
        name: bc_name.to_string(),
        data: bc_bytes,
        relocs: Vec::new(),
        debug_rows: Vec::new(),
    });

    // 4. Append .llvmcmd / __LLVM,__cmdline.
    obj.sections.push(Section {
        name: cmd_name.to_string(),
        data: cmdline.as_bytes().to_vec(),
        relocs: Vec::new(),
        debug_rows: Vec::new(),
    });

    Ok(())
}

/// Return the `(bitcode_section_name, cmdline_section_name)` pair for `fmt`.
fn section_names(fmt: ObjectFormat) -> (&'static str, &'static str) {
    match fmt {
        ObjectFormat::MachO => ("__LLVM,__bitcode", "__LLVM,__cmdline"),
        // ELF and COFF both use the same LLVM ThinLTO section names.
        ObjectFormat::Elf | ObjectFormat::Coff => (".llvmbc", ".llvmcmd"),
    }
}
