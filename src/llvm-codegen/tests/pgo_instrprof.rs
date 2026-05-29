//! Tests for PGO instrprof intrinsic no-op lowering (issue #219).
//!
//! Verifies that `llvm.instrprof.increment` and `llvm.instrprof.value.profile`
//! compile without error and are lowered to NOP machine instructions.

use llvm_codegen::{
    emit_object,
    isel::IselBackend,
    regalloc::{
        allocate_registers, apply_allocation, compute_live_intervals, insert_spill_reloads,
        RegAllocStrategy,
    },
    ObjectFormat,
};
use llvm_ir::InstrprofIntrinsic;
use llvm_ir_parser::parser::parse;
use llvm_target_x86::{
    instructions::{CALL_R, MOV_LOAD_MR, MOV_STORE_RM, NOP},
    X86Backend, X86Emitter,
};

// ── helpers ──────────────────────────────────────────────────────────────────

fn compile_x86(src: &str) -> Vec<llvm_codegen::isel::MInstr> {
    let (ctx, module) = parse(src).expect("parse failed");
    let func = module
        .functions
        .iter()
        .find(|f| f.name == "main" && !f.is_declaration)
        .expect("no @main definition");
    let mut backend = X86Backend::default();
    let mut mf = backend.lower_function(&ctx, &module, func);
    let intervals = compute_live_intervals(&mf);
    let mut result = allocate_registers(
        &intervals,
        &mf.allocatable_pregs,
        &mf.allocatable_fp_pregs,
        RegAllocStrategy::LinearScan,
    );
    insert_spill_reloads(
        &mut mf,
        &mut result,
        MOV_LOAD_MR,
        MOV_STORE_RM,
        MOV_LOAD_MR,
        MOV_STORE_RM,
    );
    apply_allocation(&mut mf, &result);
    mf.blocks
        .iter()
        .flat_map(|b| b.instrs.iter().cloned())
        .collect()
}

fn compile_x86_object(src: &str) -> Vec<u8> {
    let (ctx, module) = parse(src).expect("parse failed");
    let func = module
        .functions
        .iter()
        .find(|f| f.name == "main" && !f.is_declaration)
        .expect("no @main definition");
    let mut backend = X86Backend::default();
    let mut mf = backend.lower_function(&ctx, &module, func);
    let intervals = compute_live_intervals(&mf);
    let mut result = allocate_registers(
        &intervals,
        &mf.allocatable_pregs,
        &mf.allocatable_fp_pregs,
        RegAllocStrategy::LinearScan,
    );
    insert_spill_reloads(
        &mut mf,
        &mut result,
        MOV_LOAD_MR,
        MOV_STORE_RM,
        MOV_LOAD_MR,
        MOV_STORE_RM,
    );
    apply_allocation(&mut mf, &result);
    let mut emitter = X86Emitter::new(ObjectFormat::Elf);
    emit_object(&mf, &mut emitter).to_bytes()
}

// ── unit tests for InstrprofIntrinsic::from_name ──────────────────────────

#[test]
fn instrprof_from_name_recognizes_increment() {
    assert_eq!(
        InstrprofIntrinsic::from_name("llvm.instrprof.increment"),
        Some(InstrprofIntrinsic::Increment)
    );
}

#[test]
fn instrprof_from_name_recognizes_value_profile() {
    assert_eq!(
        InstrprofIntrinsic::from_name("llvm.instrprof.value.profile"),
        Some(InstrprofIntrinsic::ValueProfile)
    );
}

#[test]
fn instrprof_from_name_returns_none_for_unknown() {
    assert_eq!(
        InstrprofIntrinsic::from_name("llvm.instrprof.unknown"),
        None
    );
    assert_eq!(InstrprofIntrinsic::from_name("llvm.lifetime.start"), None);
    assert_eq!(InstrprofIntrinsic::from_name("llvm.vp.add.i32"), None);
    assert_eq!(InstrprofIntrinsic::from_name(""), None);
}

// ── backend lowering tests ─────────────────────────────────────────────────

const INSTRPROF_INCREMENT_IR: &str = r#"
declare void @llvm.instrprof.increment(ptr, i64, i32, i32)
@__profn_foo = external global i8

define i32 @main() {
entry:
  call void @llvm.instrprof.increment(ptr @__profn_foo, i64 12345, i32 1, i32 0)
  ret i32 0
}
"#;

const INSTRPROF_VALUE_PROFILE_IR: &str = r#"
declare void @llvm.instrprof.value.profile(ptr, i64, i64, i32, i32)
@__profn_bar = external global i8

define i32 @main() {
entry:
  %v = add i64 0, 42
  call void @llvm.instrprof.value.profile(ptr @__profn_bar, i64 99, i64 %v, i32 0, i32 0)
  ret i32 0
}
"#;

const INSTRPROF_SURROUNDING_ARITHMETIC_IR: &str = r#"
declare void @llvm.instrprof.increment(ptr, i64, i32, i32)
@__profn_arith = external global i8

define i32 @main() {
entry:
  %a = add i32 3, 4
  call void @llvm.instrprof.increment(ptr @__profn_arith, i64 1, i32 1, i32 0)
  %b = add i32 %a, 5
  ret i32 %b
}
"#;

#[test]
fn instrprof_increment_compiles_without_error() {
    // Must not panic; object bytes must be non-empty.
    let bytes = compile_x86_object(INSTRPROF_INCREMENT_IR);
    assert!(
        !bytes.is_empty(),
        "expected non-empty object for instrprof.increment"
    );
}

#[test]
fn instrprof_value_profile_compiles_without_error() {
    let bytes = compile_x86_object(INSTRPROF_VALUE_PROFILE_IR);
    assert!(
        !bytes.is_empty(),
        "expected non-empty object for instrprof.value.profile"
    );
}

#[test]
fn instrprof_increment_emits_nop_not_call() {
    // The intrinsic must lower to NOP, not to a CALL_R instruction.
    let instrs = compile_x86(INSTRPROF_INCREMENT_IR);
    let has_nop = instrs.iter().any(|mi| mi.opcode == NOP);
    let has_call_to_instrprof = instrs.iter().any(|mi| mi.opcode == CALL_R);
    assert!(
        has_nop,
        "expected at least one NOP from instrprof.increment lowering"
    );
    // We allow call_r for other calls but confirm instrprof didn't sneak through as a real call.
    // Since @main has no other calls, any CALL_R would be the intrinsic leaking through.
    assert!(
        !has_call_to_instrprof,
        "instrprof.increment must not emit a CALL_R — it should be a NOP"
    );
}

#[test]
fn instrprof_does_not_disturb_surrounding_arithmetic() {
    // IR: add, instrprof.increment, add, ret.
    // After lowering the two adds must still produce ADD_RR instructions.
    use llvm_target_x86::instructions::ADD_RR;
    let instrs = compile_x86(INSTRPROF_SURROUNDING_ARITHMETIC_IR);
    let add_count = instrs.iter().filter(|mi| mi.opcode == ADD_RR).count();
    assert!(
        add_count >= 2,
        "expected at least 2 ADD_RR instructions around the instrprof NOP, got {add_count}"
    );
}
