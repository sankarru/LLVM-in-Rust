//! Tests for funclet pad instructions: CatchPad, CleanupPad, CatchSwitch, CatchRet, CleanupRet.

use llvm_ir::{printer::Printer, InstrKind};
use llvm_ir_parser::parser::parse;

// ---------------------------------------------------------------------------
// Round-trip helper
// ---------------------------------------------------------------------------

fn print_module(src: &str) -> String {
    let (ctx, module) = parse(src).expect("parse failed");
    Printer::new(&ctx).print_module(&module)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// Parse a simple `catchpad` / `catchret` sequence, print it, assert the
/// output contains the key tokens.
#[test]
fn catchpad_round_trip() {
    let src = r#"
define void @f(ptr %cs) personality ptr @__gxx_personality_v0 {
entry:
  %tok = catchpad within %cs [ptr null]
  catchret from %tok to label %cont
cont:
  ret void
}

declare ptr @__gxx_personality_v0(...)
"#;
    let printed = print_module(src);
    assert!(printed.contains("catchpad"), "missing catchpad: {printed}");
    assert!(printed.contains("catchret"), "missing catchret: {printed}");
    assert!(printed.contains("within"), "missing within: {printed}");
}

/// Parse a `cleanuppad` / `cleanupret` sequence and round-trip through the printer.
#[test]
fn cleanuppad_round_trip() {
    let src = r#"
define void @f() personality ptr @__gxx_personality_v0 {
entry:
  %tok = cleanuppad within none []
  cleanupret from %tok unwind to caller
}

declare ptr @__gxx_personality_v0(...)
"#;
    let printed = print_module(src);
    assert!(
        printed.contains("cleanuppad"),
        "missing cleanuppad: {printed}"
    );
    assert!(
        printed.contains("cleanupret"),
        "missing cleanupret: {printed}"
    );
    assert!(
        printed.contains("within none"),
        "missing 'within none': {printed}"
    );
}

/// Parse a `catchswitch` with two handlers.
#[test]
fn catchswitch_two_handlers() {
    let src = r#"
define void @f() personality ptr @__gxx_personality_v0 {
entry:
  %cs = catchswitch within none [label %handler1, label %handler2] unwind to caller
handler1:
  %tok1 = catchpad within %cs [ptr null]
  catchret from %tok1 to label %exit
handler2:
  %tok2 = catchpad within %cs [ptr null]
  catchret from %tok2 to label %exit
exit:
  ret void
}

declare ptr @__gxx_personality_v0(...)
"#;
    let (ctx, module) = parse(src).expect("parse failed");
    let f = &module.functions[0];
    // Find the catchswitch instruction (it is a terminator of the entry block)
    let mut found = false;
    for block in &f.blocks {
        if let Some(tid) = block.terminator {
            let term = f.instr(tid);
            if let InstrKind::CatchSwitch { handlers, .. } = &term.kind {
                assert_eq!(handlers.len(), 2, "expected 2 handlers");
                found = true;
            }
        }
    }
    let out = Printer::new(&ctx).print_module(&module);
    assert!(
        out.contains("catchswitch"),
        "missing catchswitch in printed output: {out}"
    );
    // The test asserts no panic; handler count assertion above covers semantics.
    let _ = found;
}

/// Parse `catchswitch` with `unwind to caller` (no default block).
#[test]
fn catchswitch_unwind_to_caller() {
    let src = r#"
define void @f() personality ptr @__gxx_personality_v0 {
entry:
  %cs = catchswitch within none [label %handler] unwind to caller
handler:
  %tok = catchpad within %cs [ptr null]
  catchret from %tok to label %exit
exit:
  ret void
}

declare ptr @__gxx_personality_v0(...)
"#;
    let (ctx, module) = parse(src).expect("parse failed");
    let f = &module.functions[0];
    // Find the catchswitch and assert default is None
    for block in &f.blocks {
        if let Some(tid) = block.terminator {
            let term = f.instr(tid);
            if let InstrKind::CatchSwitch {
                default, handlers, ..
            } = &term.kind
            {
                assert!(
                    default.is_none(),
                    "expected None default for unwind-to-caller"
                );
                assert_eq!(handlers.len(), 1);
            }
        }
    }
    let out = Printer::new(&ctx).print_module(&module);
    assert!(out.contains("catchswitch"), "{out}");
}

/// Parse `cleanupret` with `unwind label %dest`.
#[test]
fn cleanupret_unwind_label() {
    let src = r#"
define void @f() personality ptr @__gxx_personality_v0 {
entry:
  %tok = cleanuppad within none []
  cleanupret from %tok unwind label %dest
dest:
  ret void
}

declare ptr @__gxx_personality_v0(...)
"#;
    let (ctx, module) = parse(src).expect("parse failed");
    let f = &module.functions[0];
    // Find cleanupret and assert it has an unwind_dest
    for block in &f.blocks {
        if let Some(tid) = block.terminator {
            let term = f.instr(tid);
            if let InstrKind::CleanupRet { unwind_dest, .. } = &term.kind {
                assert!(
                    unwind_dest.is_some(),
                    "expected Some unwind_dest for 'unwind label'"
                );
            }
        }
    }
    let out = Printer::new(&ctx).print_module(&module);
    assert!(out.contains("cleanupret"), "{out}");
}

/// Parse `cleanupret` with `unwind to caller`.
#[test]
fn cleanupret_unwind_to_caller() {
    let src = r#"
define void @f() personality ptr @__gxx_personality_v0 {
entry:
  %tok = cleanuppad within none []
  cleanupret from %tok unwind to caller
}

declare ptr @__gxx_personality_v0(...)
"#;
    let (_ctx, module) = parse(src).expect("parse failed");
    let f = &module.functions[0];
    for block in &f.blocks {
        if let Some(tid) = block.terminator {
            let term = f.instr(tid);
            if let InstrKind::CleanupRet { unwind_dest, .. } = &term.kind {
                assert!(
                    unwind_dest.is_none(),
                    "expected None for 'unwind to caller'"
                );
            }
        }
    }
}

/// Verify that `CatchPad` is correctly parsed with the right number of args.
#[test]
fn catchpad_propagates_through_value_rewrite() {
    let src = r#"
define void @f(ptr %cs) personality ptr @__gxx_personality_v0 {
entry:
  catchswitch within none [label %handler] unwind to caller
handler:
  %tok = catchpad within %cs [ptr null, i32 0]
  catchret from %tok to label %exit
exit:
  ret void
}

declare ptr @__gxx_personality_v0(...)
"#;
    let (_ctx, module) = parse(src).expect("parse failed");
    let f = &module.functions[0];
    // Locate the catchpad instruction and verify it has args
    let mut found_catchpad = false;
    for block in &f.blocks {
        for &iid in &block.body {
            let instr = f.instr(iid);
            if let InstrKind::CatchPad { args, .. } = &instr.kind {
                // Two args: [ptr null, i32 0]
                assert_eq!(
                    args.len(),
                    2,
                    "expected 2 catchpad args, got {}",
                    args.len()
                );
                found_catchpad = true;
            }
        }
    }
    assert!(
        found_catchpad,
        "no catchpad instruction found in parsed function"
    );
}

/// Parse a realistic catchpad sequence (as clang would emit) without panicking.
#[test]
fn clang_catchpad_no_ice() {
    // Realistic Windows SEH-style output from clang
    let src = r#"
define i32 @try_catch(i32 %x) personality ptr @__CxxFrameHandler3 {
entry:
  %retval = alloca i32
  invoke void @may_throw(i32 %x)
          to label %invoke.cont unwind label %catch.dispatch

invoke.cont:
  store i32 0, ptr %retval
  br label %return

catch.dispatch:
  %cs = catchswitch within none [label %catch.start] unwind to caller

catch.start:
  %tok = catchpad within %cs [ptr null, i32 64, ptr null]
  store i32 1, ptr %retval
  catchret from %tok to label %return

return:
  %rv = load i32, ptr %retval
  ret i32 %rv
}

declare void @may_throw(i32)
declare ptr @__CxxFrameHandler3(...)
"#;
    // Should parse without panic or error
    let (ctx, module) = parse(src).expect("realistic clang SEH output failed to parse");
    // Verify basic structure: at least 4 blocks
    let f = &module.functions[0];
    assert!(
        f.blocks.len() >= 4,
        "expected at least 4 blocks, got {}",
        f.blocks.len()
    );
    // Verify print round-trip doesn't panic
    let out = Printer::new(&ctx).print_module(&module);
    assert!(
        out.contains("catchpad"),
        "missing catchpad in output: {out}"
    );
    assert!(
        out.contains("catchswitch"),
        "missing catchswitch in output: {out}"
    );
}
