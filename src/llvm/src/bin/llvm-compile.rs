//! `llvm-compile` — compile an LLVM IR `.ll` file into a native `.o` object.
//!
//! Usage:
//!   llvm-compile <input.ll> -o <output.o> [-O <level>] [--target <arch>]
//!
//! Options:
//!   <input.ll>          Path to the LLVM IR source file
//!   -o <output.o>       Output object file path (required)
//!   -O <level>          Optimization level: 0, 1, 2, 3 (default: 2)
//!   --target <arch>     Target architecture: x86_64 (default, only supported)
//!
//! Examples:
//!   llvm-compile foo.ll -o foo.o
//!   llvm-compile foo.ll -o foo.o -O1
//!   clang foo.o -o foo && ./foo

use std::{env, fs, process::ExitCode};

use llvm::compile::{compile_ir_to_object, host_object_format};
use llvm_transforms::OptLevel;

fn usage() -> String {
    "usage: llvm-compile <input.ll> -o <output.o> [-O <level>] [--target <arch>]".to_string()
}

fn main() -> ExitCode {
    let mut args_iter = env::args().skip(1).peekable();

    let mut input: Option<String> = None;
    let mut output: Option<String> = None;
    let mut opt_level = OptLevel::O2;

    while let Some(arg) = args_iter.next() {
        match arg.as_str() {
            "-o" => {
                output = Some(
                    args_iter
                        .next()
                        .unwrap_or_else(|| die("-o requires an argument")),
                );
            }
            "-O" => {
                let level_str = args_iter
                    .next()
                    .unwrap_or_else(|| die("-O requires an argument"));
                opt_level = OptLevel::parse(&level_str)
                    .unwrap_or_else(|| die(&format!("unknown optimization level: {level_str}")));
            }
            "--target" => {
                let target = args_iter
                    .next()
                    .unwrap_or_else(|| die("--target requires an argument"));
                if target != "x86_64" {
                    eprintln!(
                        "warning: only x86_64 target is supported; ignoring --target {target}"
                    );
                }
            }
            s if s.starts_with("-O") => {
                let level_str = &s[2..];
                opt_level = OptLevel::parse(level_str)
                    .unwrap_or_else(|| die(&format!("unknown optimization level: {level_str}")));
            }
            s if s.starts_with('-') => {
                eprintln!("warning: unknown flag {s}, ignoring");
            }
            s => {
                if input.is_some() {
                    die("multiple input files not supported");
                }
                input = Some(s.to_owned());
            }
        }
    }

    let input = input.unwrap_or_else(|| die(&usage()));
    let output = output.unwrap_or_else(|| die("output path required (-o <output.o>)"));
    let fmt = host_object_format();

    let src = match fs::read_to_string(&input) {
        Ok(s) => s,
        Err(e) => die(&format!("cannot read {input}: {e}")),
    };

    match compile_ir_to_object(&src, opt_level, fmt) {
        Ok(bytes) => match fs::write(&output, &bytes) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => die(&format!("cannot write {output}: {e}")),
        },
        Err(e) => die(&format!("compilation failed: {e}")),
    }
}

fn die(msg: &str) -> ! {
    eprintln!("error: {msg}");
    std::process::exit(1);
}
