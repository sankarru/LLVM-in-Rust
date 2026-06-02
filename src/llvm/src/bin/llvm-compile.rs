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
//!   --production-parse-limits
//!                       Use conservative parser limits for production pilots
//!   --max-input-bytes N Maximum input size in bytes
//!   --max-functions N  Maximum functions/declarations in the module
//!   --max-blocks-per-function N
//!                       Maximum basic blocks per function
//!   --max-instructions-per-function N
//!                       Maximum instructions per function
//!   --max-type-depth N  Maximum recursive type nesting
//!   --max-constant-depth N
//!                       Maximum recursive constant nesting
//!
//! Examples:
//!   llvm-compile foo.ll -o foo.o
//!   llvm-compile foo.ll -o foo.o -O1
//!   clang foo.o -o foo && ./foo

use std::{env, fs, process::ExitCode};

use llvm::compile::{compile_ir_to_object_with_limits, host_object_format};
use llvm_ir_parser::parser::ParseLimits;
use llvm_transforms::OptLevel;

fn usage() -> String {
    "usage: llvm-compile <input.ll> -o <output.o> [-O <level>] [--target <arch>] \
     [--production-parse-limits] [--max-input-bytes N] [--max-functions N] \
     [--max-blocks-per-function N] [--max-instructions-per-function N] \
     [--max-type-depth N] [--max-constant-depth N]"
        .to_string()
}

fn main() -> ExitCode {
    let mut args_iter = env::args().skip(1).peekable();

    let mut input: Option<String> = None;
    let mut output: Option<String> = None;
    let mut opt_level = OptLevel::O2;
    let mut parse_limits = ParseLimits::unlimited();

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
            "--production-parse-limits" => {
                parse_limits = ParseLimits::production_defaults();
            }
            "--max-input-bytes" => {
                parse_limits.max_source_bytes =
                    Some(parse_limit_arg(&mut args_iter, "--max-input-bytes"));
            }
            "--max-functions" => {
                parse_limits.max_functions =
                    Some(parse_limit_arg(&mut args_iter, "--max-functions"));
            }
            "--max-blocks-per-function" => {
                parse_limits.max_blocks_per_function =
                    Some(parse_limit_arg(&mut args_iter, "--max-blocks-per-function"));
            }
            "--max-instructions-per-function" => {
                parse_limits.max_instructions_per_function = Some(parse_limit_arg(
                    &mut args_iter,
                    "--max-instructions-per-function",
                ));
            }
            "--max-type-depth" => {
                parse_limits.max_type_depth =
                    Some(parse_limit_arg(&mut args_iter, "--max-type-depth"));
            }
            "--max-constant-depth" => {
                parse_limits.max_constant_depth =
                    Some(parse_limit_arg(&mut args_iter, "--max-constant-depth"));
            }
            s if s.starts_with("--max-input-bytes=") => {
                parse_limits.max_source_bytes = Some(parse_limit_value(
                    &s["--max-input-bytes=".len()..],
                    "--max-input-bytes",
                ));
            }
            s if s.starts_with("--max-functions=") => {
                parse_limits.max_functions = Some(parse_limit_value(
                    &s["--max-functions=".len()..],
                    "--max-functions",
                ));
            }
            s if s.starts_with("--max-blocks-per-function=") => {
                parse_limits.max_blocks_per_function = Some(parse_limit_value(
                    &s["--max-blocks-per-function=".len()..],
                    "--max-blocks-per-function",
                ));
            }
            s if s.starts_with("--max-instructions-per-function=") => {
                parse_limits.max_instructions_per_function = Some(parse_limit_value(
                    &s["--max-instructions-per-function=".len()..],
                    "--max-instructions-per-function",
                ));
            }
            s if s.starts_with("--max-type-depth=") => {
                parse_limits.max_type_depth = Some(parse_limit_value(
                    &s["--max-type-depth=".len()..],
                    "--max-type-depth",
                ));
            }
            s if s.starts_with("--max-constant-depth=") => {
                parse_limits.max_constant_depth = Some(parse_limit_value(
                    &s["--max-constant-depth=".len()..],
                    "--max-constant-depth",
                ));
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

    match compile_ir_to_object_with_limits(&src, opt_level, fmt, parse_limits) {
        Ok(bytes) => match fs::write(&output, &bytes) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => die(&format!("cannot write {output}: {e}")),
        },
        Err(e) => die(&format!("compilation failed: {e}")),
    }
}

fn parse_limit_arg<I>(args: &mut std::iter::Peekable<I>, flag: &str) -> usize
where
    I: Iterator<Item = String>,
{
    let value = args
        .next()
        .unwrap_or_else(|| die(&format!("{flag} requires an argument")));
    parse_limit_value(&value, flag)
}

fn parse_limit_value(value: &str, flag: &str) -> usize {
    let parsed = value
        .parse::<usize>()
        .unwrap_or_else(|_| die(&format!("{flag} requires a positive integer")));
    if parsed == 0 {
        die(&format!("{flag} must be greater than zero"));
    }
    parsed
}

fn die(msg: &str) -> ! {
    eprintln!("error: {msg}");
    std::process::exit(1);
}
