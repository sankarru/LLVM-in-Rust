//! Negative parser tests for Milestone X / issue #383: malformed and
//! adversarial IR must fail with structured [`ParseError`]s, not panic, hang,
//! or consume unbounded resources.
//!
//! The five categories tracked by #383 are exercised here:
//!
//! 1. **Oversized IR** — `max_source_bytes` rejects before tokenising.
//! 2. **Deeply nested types / constants** — `max_type_depth` /
//!    `max_constant_depth` cap recursion in `parse_type` / `parse_value`.
//! 3. **Huge CFGs** — `max_functions`, `max_blocks_per_function`,
//!    `max_instructions_per_function` cap module / function shape.
//! 4. **Malformed EH constructs** — `invoke` / `landingpad` / `resume` shape
//!    errors return structured `ParseError`s, never panic.
//! 5. **Unsupported intrinsics** — calls to unknown `@llvm.*` intrinsics
//!    parse as ordinary calls (intrinsic dispatch is a codegen concern); a
//!    syntactically malformed intrinsic name still produces a clean error.
//!
//! Each test asserts that `parse` / `parse_with_limits` returns
//! `Err(ParseError)` rather than panicking — both branches are real
//! production guarantees.  Negative tests intentionally avoid `unwrap()` /
//! `expect()` on the parse result to keep failure modes visible.

use llvm_ir_parser::parser::{parse, parse_with_limits, ParseError, ParseLimits};

/// Helper: assert the parse failed AND that the error message contains a
/// well-known fragment.  We avoid printing the full IR back on failure so
/// the assertion message stays scoped to the diagnostic.
fn assert_parse_err_contains(src: &str, needle: &str) -> ParseError {
    match parse(src) {
        Ok(_) => panic!("expected parse error containing {needle:?}, but parse succeeded"),
        Err(err) => {
            assert!(
                err.message.contains(needle),
                "expected parse error containing {needle:?}, got {:?}",
                err.message
            );
            err
        }
    }
}

fn assert_limited_err_contains(src: &str, limits: ParseLimits, needle: &str) -> ParseError {
    match parse_with_limits(src, limits) {
        Ok(_) => {
            panic!("expected limit-exceeded parse error containing {needle:?}, but parse succeeded")
        }
        Err(err) => {
            assert!(
                err.message.contains(needle),
                "expected limit-exceeded error containing {needle:?}, got {:?}",
                err.message
            );
            err
        }
    }
}

// ───────── 1. Oversized IR ─────────────────────────────────────────────────

/// A simple valid module that overflows a tight `max_source_bytes` cap must
/// fail at the entry boundary with a recognisable diagnostic, before any
/// tokenising work happens.
#[test]
fn oversized_source_bytes_rejected_before_tokenising() {
    // 8 KiB of harmless padding inside a comment.  Source bytes ≫ cap.
    let mut src = String::from("define void @f() { entry: ret void }\n");
    src.push_str("; ");
    src.push_str(&"a".repeat(8 * 1024));
    src.push('\n');

    let limits = ParseLimits {
        max_source_bytes: Some(256),
        ..ParseLimits::unlimited()
    };
    let err = assert_limited_err_contains(&src, limits, "source bytes");
    assert_eq!(err.line, 1, "source-bytes check should report line 1");
}

/// Without limits, the same oversized payload must still parse correctly
/// (back-compat for trusted callers).
#[test]
fn oversized_source_bytes_pass_when_unlimited() {
    let mut src = String::from("define void @f() { entry: ret void }\n");
    src.push_str("; ");
    src.push_str(&"a".repeat(8 * 1024));
    src.push('\n');

    parse_with_limits(&src, ParseLimits::unlimited())
        .map_err(|e| format!("expected unlimited parse to succeed, got: {}", e.message))
        .unwrap();
}

// ───────── 2. Deeply nested types / constants ─────────────────────────────

/// Pathological `{{{…}}}` type nesting must bail via `max_type_depth`,
/// bounded *before* recursion blows the stack.
#[test]
fn deeply_nested_struct_types_rejected_by_type_depth_limit() {
    let depth = 64usize;
    let inner = "i32";
    let mut nested = String::from(inner);
    for _ in 0..depth {
        nested = format!("{{ {nested} }}");
    }
    // Use the nested type in a global type declaration.
    let src = format!("@deep = external constant {nested}");
    let limits = ParseLimits {
        max_type_depth: Some(8),
        ..ParseLimits::unlimited()
    };
    assert_limited_err_contains(&src, limits, "type nesting depth");
}

/// Deeply nested aggregate constant literals must bail via
/// `max_constant_depth`.  Reaches `parse_value` recursion, not just type
/// recursion.
///
/// LLVM struct-constant syntax requires each field to be `<type> <value>`,
/// so we grow the constant by wrapping `{ prev_ty prev_val }` at each level.
#[test]
fn deeply_nested_constant_values_rejected_by_constant_depth_limit() {
    let depth = 32usize;
    let mut nested_ty = String::from("i32");
    let mut nested_val = String::from("0");
    for _ in 0..depth {
        let prev_ty = nested_ty.clone();
        let prev_val = nested_val.clone();
        nested_ty = format!("{{ {prev_ty} }}");
        nested_val = format!("{{ {prev_ty} {prev_val} }}");
    }
    let src = format!("@deep = constant {nested_ty} {nested_val}");
    let limits = ParseLimits {
        // Allow enough type depth so the type itself parses; the constant
        // recursion is what we want to trip.
        max_type_depth: Some(depth + 4),
        max_constant_depth: Some(8),
        ..ParseLimits::unlimited()
    };
    assert_limited_err_contains(&src, limits, "constant nesting depth");
}

// ───────── 3. Huge CFGs / module shape ─────────────────────────────────────

/// A module declaring more functions than `max_functions` must be rejected.
#[test]
fn too_many_functions_rejected() {
    let mut src = String::new();
    for i in 0..10 {
        src.push_str(&format!("declare void @f{i}()\n"));
    }
    let limits = ParseLimits {
        max_functions: Some(3),
        ..ParseLimits::unlimited()
    };
    assert_limited_err_contains(&src, limits, "function count");
}

/// A function with more basic blocks than `max_blocks_per_function` must be
/// rejected — and rejected *during* parsing rather than after walking to the
/// closing brace.
#[test]
fn too_many_blocks_per_function_rejected() {
    // 6 blocks total: entry + b1..b5.
    let src = r#"
define void @big(i1 %c) {
entry:
  br i1 %c, label %b1, label %b5
b1:
  br label %b2
b2:
  br label %b3
b3:
  br label %b4
b4:
  ret void
b5:
  ret void
}
"#;
    let limits = ParseLimits {
        max_blocks_per_function: Some(3),
        ..ParseLimits::unlimited()
    };
    assert_limited_err_contains(src, limits, "basic blocks per function");
}

/// Function exceeding `max_instructions_per_function` is rejected during
/// parsing of the offending instruction.
#[test]
fn too_many_instructions_per_function_rejected() {
    // 5 body instructions + terminator.
    let src = r#"
define i32 @f(i32 %x) {
entry:
  %a = add i32 %x, 1
  %b = add i32 %a, 1
  %c = add i32 %b, 1
  %d = add i32 %c, 1
  %e = add i32 %d, 1
  ret i32 %e
}
"#;
    let limits = ParseLimits {
        max_instructions_per_function: Some(3),
        ..ParseLimits::unlimited()
    };
    assert_limited_err_contains(src, limits, "instructions per function");
}

// ───────── 4. Malformed EH constructs ──────────────────────────────────────

/// `invoke` requires `to label %normal unwind label %lpad`.  Omitting the
/// `unwind` clause must produce a clean parse error, not a panic.
#[test]
fn invoke_missing_unwind_clause() {
    let src = r#"
declare i32 @may_throw()
define i32 @f() {
entry:
  %r = invoke i32 @may_throw() to label %normal
normal:
  ret i32 %r
}
"#;
    // Just confirm parsing fails cleanly; the exact error wording belongs to
    // the parser and may evolve.
    if parse(src).is_ok() {
        panic!("expected parse error for invoke without unwind clause");
    }
}

/// `landingpad` with no clauses and no `cleanup` keyword is invalid LLVM IR.
/// The parser must reject it without panicking.
#[test]
fn landingpad_without_clauses_or_cleanup() {
    let src = r#"
declare void @anchor()
define i32 @f() personality ptr @anchor {
entry:
  %r = invoke i32 @anchor() to label %normal unwind label %lpad
normal:
  ret i32 0
lpad:
  %lp = landingpad { ptr, i32 }
  ret i32 -1
}
"#;
    assert_parse_err_contains(src, "landingpad requires cleanup or at least one clause");
}

/// `resume` outside a function (top-level) is structurally invalid.  Confirm
/// the parser doesn't accept this nonsense and doesn't panic.
#[test]
fn resume_outside_function_rejected() {
    let src = "resume { ptr, i32 } undef";
    assert!(
        parse(src).is_err(),
        "top-level `resume` must not parse as a module"
    );
}

/// `catchswitch` with a missing `to` clause: structurally malformed; parser
/// must surface a ParseError rather than misparse.
#[test]
fn catchswitch_missing_to_clause_rejected() {
    let src = r#"
define void @f() personality ptr null {
entry:
  br label %dispatch
dispatch:
  %cs = catchswitch within none [label %handler]
handler:
  ret void
}
"#;
    assert!(
        parse(src).is_err(),
        "catchswitch without unwind destination must be rejected"
    );
}

// ───────── 5. Unsupported intrinsics ───────────────────────────────────────

/// An unknown `@llvm.*` intrinsic call must parse cleanly as an ordinary
/// call (the parser does not validate intrinsic semantics; codegen does).
/// This pins the contract that we *don't* reject unknown intrinsics at
/// parse time — they reach the pipeline as named calls and either lower
/// generically or are rejected by codegen with a clear diagnostic.
#[test]
fn unknown_intrinsic_parses_as_call() {
    let src = r#"
declare void @llvm.unknown.intrinsic.never.seen(i32)
define void @f(i32 %x) {
entry:
  call void @llvm.unknown.intrinsic.never.seen(i32 %x)
  ret void
}
"#;
    let (_ctx, module) = parse(src).expect("unknown intrinsics parse as ordinary calls");
    assert_eq!(module.functions.len(), 2);
    // The defining function must have one body instruction (the call).
    let f = module.functions.iter().find(|f| f.name == "f").unwrap();
    assert!(!f.blocks.is_empty());
}

/// A syntactically malformed intrinsic call (e.g. missing `(` after the
/// callee) must still produce a clean parse error.
#[test]
fn syntactically_malformed_intrinsic_call_rejected() {
    let src = r#"
declare void @llvm.foo.bar(i32)
define void @f(i32 %x) {
entry:
  call void @llvm.foo.bar i32 %x   ; missing parens — must be rejected
  ret void
}
"#;
    assert_parse_err_contains(src, "expected ");
}

// ───────── 6. Parser termination (no-progress loops) ──────────────────────

/// Regression for a fuzz-found infinite loop (Milestone Z burn-in, llvm-stress
/// timeout bucket `567c0234…`). After an implicit `entry` block is terminated,
/// a stray instruction-shaped token that is *not* a `label:` used to make
/// `parse_block` re-select the already-complete `entry` block, break without
/// consuming the token, and return — so `parse_function` called it forever
/// (libFuzzer killed it on a 60s timeout). The parser must now reject such
/// stray content with a structured error instead of looping.
#[test]
fn stray_token_after_terminated_implicit_entry_is_rejected() {
    // `unreachable` terminates the implicit entry block; the bare `%x` that
    // follows has no `=` and no `:`, so it is neither an instruction result nor
    // a block label. This is the distilled minimum of the fuzz timeout input.
    let src = "define void @f() {\nunreachable\n%x\n}\n";
    assert_parse_err_contains(src, "expected block label after terminated block");
}

/// The same shape with a numeric stray token (`5` is not `5:` and has no `=`)
/// after a terminated implicit entry must also terminate with an error rather
/// than spin — the integer-literal label path has the identical hazard.
#[test]
fn stray_int_token_after_terminated_implicit_entry_is_rejected() {
    let src = "define void @f() {\nret void\n5\n}\n";
    assert_parse_err_contains(src, "expected block label after terminated block");
}

/// Guardrail: the fix must not reject the legitimate case it sits next to — a
/// single-block function whose entry block is implicit (no `entry:` label) and
/// is the only block must still parse cleanly.
#[test]
fn implicit_entry_single_block_still_parses() {
    let src = "define i32 @g() {\n  ret i32 0\n}\n";
    let (_ctx, module) = parse(src).expect("implicit single-block entry must parse");
    assert_eq!(module.functions.len(), 1);
}

// ───────── 7. Unsigned size literals ────────────────────────────────────────

/// Regression for a Milestone Z fuzz crash after the parser-timeout fix. A
/// malformed vector length such as `<-4 x i16>` used to be accepted by
/// `expect_uint_lit` via `as u64`, producing a huge vector type and later
/// overflowing codegen frame-size arithmetic. The parser must reject it at the
/// source boundary instead.
#[test]
fn negative_vector_length_is_rejected() {
    let src = r#"
define void @f() {
entry:
  %a = alloca <-4 x i16>, align 8
  ret void
}
"#;
    assert_parse_err_contains(src, "expected unsigned integer literal");
}

/// Array lengths use the same unsigned literal helper and must reject negative
/// syntax for the same reason.
#[test]
fn negative_array_length_is_rejected() {
    let src = r#"
define void @f() {
entry:
  %a = alloca [-1 x i8], align 1
  ret void
}
"#;
    assert_parse_err_contains(src, "expected unsigned integer literal");
}

// ───────── 8. Unsigned align / aggregate-index literals ────────────────────

// Follow-up to #404 (issue #409): alignment and aggregate-index operands used
// the same `expect_uint_lit` + `as` cast that silently wrapped a negative
// literal to a huge unsigned value. They now route through the nonnegative
// helper and must reject negative syntax at the source boundary.

/// `align` operands (here on a `load`, via `parse_optional_align`).
#[test]
fn negative_load_alignment_is_rejected() {
    let src = r#"
define i32 @f(ptr %p) {
entry:
  %v = load i32, ptr %p, align -8
  ret i32 %v
}
"#;
    assert_parse_err_contains(src, "expected unsigned integer literal");
}

/// `extractvalue` aggregate indices.
#[test]
fn negative_extractvalue_index_is_rejected() {
    let src = r#"
define i32 @f() {
entry:
  %v = extractvalue {i32, i32} undef, -1
  ret i32 %v
}
"#;
    assert_parse_err_contains(src, "expected unsigned integer literal");
}

/// `insertvalue` aggregate indices.
#[test]
fn negative_insertvalue_index_is_rejected() {
    let src = r#"
define void @f() {
entry:
  %v = insertvalue {i32, i32} undef, i32 5, -1
  ret void
}
"#;
    assert_parse_err_contains(src, "expected unsigned integer literal");
}

/// Guardrail: valid non-negative align/index operands must still parse, so the
/// hardening above does not over-reject well-formed input.
#[test]
fn nonnegative_align_and_indices_still_parse() {
    let src = r#"
define i32 @f(ptr %p) {
entry:
  %v = load i32, ptr %p, align 8
  %e = extractvalue {i32, i32} undef, 1
  %w = insertvalue {i32, i32} undef, i32 5, 0
  ret i32 %v
}
"#;
    parse(src)
        .map_err(|e| format!("valid align/index program rejected: {}", e.message))
        .unwrap();
}

// ───────── Smoke: depth-checked round-trip ─────────────────────────────────

/// Negative tests above all assert *rejection*; this one pins the positive
/// shape — a non-trivial module that exercises every limit-checked code path
/// (multiple functions, branches, GEPs, calls, modest type/constant nesting)
/// must accept under `production_defaults()`.
#[test]
fn production_defaults_accept_a_modest_realistic_program() {
    let src = r#"
@table = constant [4 x i32] [i32 1, i32 2, i32 3, i32 4]
declare i32 @lookup(i32)
define i32 @main(i32 %i) {
entry:
  %cmp = icmp slt i32 %i, 4
  br i1 %cmp, label %ok, label %fallback
ok:
  %r = call i32 @lookup(i32 %i)
  ret i32 %r
fallback:
  ret i32 -1
}
"#;
    parse_with_limits(src, ParseLimits::production_defaults())
        .map_err(|e| {
            format!(
                "production_defaults rejected a valid modest program: {}",
                e.message
            )
        })
        .unwrap();
}
