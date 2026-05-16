//! `llvm-ir-compile` — compile a `.ll` file to a native object file.
//!
//! Usage:
//!   llvm-ir-compile <input.ll> -o <output.o> [--no-mem2reg]
//!
//! Platform dispatch (same as smoke.rs):
//!   macOS aarch64 → AArch64Backend + MachO
//!   everything else → X86Backend + ELF

use std::process;

use llvm_codegen::{
    emit_module_object,
    isel::IselBackend,
    regalloc::{apply_allocation, compute_live_intervals, insert_spill_reloads, linear_scan},
    ObjectFormat,
};
use llvm_ir_parser::parser::parse;
use llvm_transforms::{pass::PassManager, Mem2Reg};

// AArch64 backend (macOS Apple Silicon)
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
use llvm_target_arm::{
    instructions::{LDR_FP, STR_FP},
    AArch64Backend, AArch64Emitter,
};

// x86-64 backend (Linux / everything else)
#[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
use llvm_target_x86::{
    instructions::{MOV_LOAD_MR, MOV_STORE_RM},
    X86Backend, X86Emitter,
};

fn usage() -> ! {
    eprintln!("Usage: llvm-ir-compile <input.ll> -o <output.o> [--no-mem2reg]");
    process::exit(1);
}

fn main() {
    let args: Vec<String> = std::env::args().collect();

    let mut input: Option<String> = None;
    let mut output: Option<String> = None;
    let mut run_mem2reg = true;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "-o" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("error: -o requires an argument");
                    usage();
                }
                output = Some(args[i].clone());
            }
            "--no-mem2reg" => {
                run_mem2reg = false;
            }
            arg if arg.starts_with('-') => {
                eprintln!("error: unknown flag '{arg}'");
                usage();
            }
            arg => {
                if input.is_some() {
                    eprintln!("error: multiple input files specified");
                    usage();
                }
                input = Some(arg.to_string());
            }
        }
        i += 1;
    }

    let input_path = input.unwrap_or_else(|| usage());
    let output_path = output.unwrap_or_else(|| usage());

    // Read source
    let src = std::fs::read_to_string(&input_path).unwrap_or_else(|e| {
        eprintln!("error: cannot read '{input_path}': {e}");
        process::exit(1);
    });

    // Parse
    let (mut ctx, mut module) = parse(&src).unwrap_or_else(|e| {
        eprintln!("error: parse failed: {e}");
        process::exit(1);
    });

    // Optionally run mem2reg
    if run_mem2reg {
        let mut pm = PassManager::new();
        pm.add_function_pass(Mem2Reg);
        pm.run(&mut ctx, &mut module);
    }

    // Collect non-declaration functions
    let functions: Vec<&llvm_ir::Function> = module
        .functions
        .iter()
        .filter(|f| !f.is_declaration)
        .collect();

    if functions.is_empty() {
        eprintln!("warning: no functions to compile (module has only declarations)");
    }

    // Lower, regalloc, and emit all functions into one object
    let obj_bytes = compile_module(&ctx, &module, &functions);

    // Write output
    std::fs::write(&output_path, &obj_bytes).unwrap_or_else(|e| {
        eprintln!("error: cannot write '{output_path}': {e}");
        process::exit(1);
    });
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn compile_module(
    ctx: &llvm_ir::Context,
    module: &llvm_ir::Module,
    functions: &[&llvm_ir::Function],
) -> Vec<u8> {
    let mut backend = AArch64Backend;
    let mut machine_fns = Vec::new();

    for func in functions {
        let mut mf = backend.lower_function(ctx, module, func);
        let intervals = compute_live_intervals(&mf);
        let mut result = linear_scan(&intervals, &mf.allocatable_pregs);
        insert_spill_reloads(&mut mf, &mut result, LDR_FP, STR_FP);
        apply_allocation(&mut mf, &result);
        machine_fns.push(mf);
    }

    let mut emitter = AArch64Emitter::new(ObjectFormat::MachO);
    emit_module_object(&machine_fns, &mut emitter).to_bytes()
}

#[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
fn compile_module(
    ctx: &llvm_ir::Context,
    module: &llvm_ir::Module,
    functions: &[&llvm_ir::Function],
) -> Vec<u8> {
    let mut backend = X86Backend;
    let mut machine_fns = Vec::new();

    for func in functions {
        let mut mf = backend.lower_function(ctx, module, func);
        let intervals = compute_live_intervals(&mf);
        let mut result = linear_scan(&intervals, &mf.allocatable_pregs);
        insert_spill_reloads(&mut mf, &mut result, MOV_LOAD_MR, MOV_STORE_RM);
        apply_allocation(&mut mf, &result);
        machine_fns.push(mf);
    }

    let mut emitter = X86Emitter::new(ObjectFormat::Elf);
    emit_module_object(&machine_fns, &mut emitter).to_bytes()
}
