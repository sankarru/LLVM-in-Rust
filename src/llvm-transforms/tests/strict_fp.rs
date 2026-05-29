//! Tests for strict IEEE-754 FP mode — barriers in GVN, LICM, and
//! constant folding, plus `strictfp` function attribute round-trip.

use llvm_ir::{
    ArgId, BlockId, Builder, Context, FastMathFlags, InstrKind, Instruction, Linkage, Module,
    TypeId, ValueRef,
};
use llvm_transforms::{gvn::Gvn, licm::Licm, pass::FunctionPass};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build a minimal f64 two-arg function `f(f64 %a, f64 %b) -> f64`.
fn make_f64_two_arg_fn() -> (Context, Module) {
    let mut ctx = Context::new();
    let mut module = Module::new("test");
    let f64_ty = ctx.f64_ty;
    let mut b = Builder::new(&mut ctx, &mut module);
    b.add_function(
        "f",
        f64_ty,
        vec![f64_ty, f64_ty],
        vec!["a".into(), "b".into()],
        false,
        Linkage::External,
    );
    let entry = b.add_block("entry");
    b.position_at_end(entry);
    let _ = b;
    (ctx, module)
}

/// Append a non-terminator instruction to block 0 of function 0.
fn push_instr(module: &mut Module, name: &str, ty: TypeId, kind: InstrKind) -> ValueRef {
    let f = &mut module.functions[0];
    let iid = f.alloc_instr(Instruction::new(Some(name.into()), ty, kind));
    f.blocks[0].body.push(iid);
    ValueRef::Instruction(iid)
}

/// Set the `ret` terminator of block 0, function 0.
fn set_ret(ctx: &Context, module: &mut Module, val: ValueRef) {
    let void_ty = ctx.void_ty;
    let f = &mut module.functions[0];
    let tid = f.alloc_instr(Instruction::new(
        None,
        void_ty,
        InstrKind::Ret { val: Some(val) },
    ));
    f.blocks[0].set_terminator(tid);
}

/// Count body (non-terminator) instructions in block `bid` of function 0.
fn body_len(module: &Module, bid: BlockId) -> usize {
    module.functions[0].blocks[bid.0 as usize].body.len()
}

fn flags_none() -> FastMathFlags {
    FastMathFlags::default()
}

fn flags_reassoc() -> FastMathFlags {
    FastMathFlags {
        reassoc: true,
        ..Default::default()
    }
}

// ---------------------------------------------------------------------------
// 1. GVN does NOT CSE two identical fadds with no fast-math flags
// ---------------------------------------------------------------------------

#[test]
fn gvn_does_not_cse_strict_fadd() {
    let (mut ctx, mut module) = make_f64_two_arg_fn();
    let f64_ty = ctx.f64_ty;
    let a = ValueRef::Argument(ArgId(0));
    let b = ValueRef::Argument(ArgId(1));

    // %x1 = fadd double %a, %b    (no flags)
    let _x1 = push_instr(
        &mut module,
        "x1",
        f64_ty,
        InstrKind::FAdd {
            flags: flags_none(),
            lhs: a,
            rhs: b,
        },
    );
    // %x2 = fadd double %a, %b    (identical, no flags)
    let x2 = push_instr(
        &mut module,
        "x2",
        f64_ty,
        InstrKind::FAdd {
            flags: flags_none(),
            lhs: a,
            rhs: b,
        },
    );
    set_ret(&ctx, &mut module, x2);

    // Before GVN: 2 body instructions.
    assert_eq!(body_len(&module, BlockId(0)), 2, "before GVN: 2 fadds");

    let mut pass = Gvn;
    let changed = pass.run_on_function(&mut ctx, &mut module.functions[0]);

    // GVN must NOT fire: strict FP forbids CSE.
    assert!(!changed, "GVN must not CSE strict (no-FMF) fadds");
    assert_eq!(
        body_len(&module, BlockId(0)),
        2,
        "both fadds must remain after GVN"
    );
}

// ---------------------------------------------------------------------------
// 2. GVN DOES CSE two identical fadds that carry reassoc
// ---------------------------------------------------------------------------

#[test]
fn gvn_cse_fadd_with_reassoc() {
    let (mut ctx, mut module) = make_f64_two_arg_fn();
    let f64_ty = ctx.f64_ty;
    let a = ValueRef::Argument(ArgId(0));
    let b = ValueRef::Argument(ArgId(1));

    // %x1 = fadd reassoc double %a, %b
    let _x1 = push_instr(
        &mut module,
        "x1",
        f64_ty,
        InstrKind::FAdd {
            flags: flags_reassoc(),
            lhs: a,
            rhs: b,
        },
    );
    // %x2 = fadd reassoc double %a, %b  (identical)
    let x2 = push_instr(
        &mut module,
        "x2",
        f64_ty,
        InstrKind::FAdd {
            flags: flags_reassoc(),
            lhs: a,
            rhs: b,
        },
    );
    set_ret(&ctx, &mut module, x2);

    assert_eq!(body_len(&module, BlockId(0)), 2, "before GVN: 2 fadds");

    let mut pass = Gvn;
    let changed = pass.run_on_function(&mut ctx, &mut module.functions[0]);

    // GVN must fire: reassoc flag allows CSE.
    assert!(changed, "GVN must CSE reassoc fadds");
    assert_eq!(
        body_len(&module, BlockId(0)),
        1,
        "second reassoc fadd must be eliminated by GVN"
    );
}

// ---------------------------------------------------------------------------
// 3. LICM does NOT hoist a strict fadd (no FMF)
// ---------------------------------------------------------------------------

/// Build a simple loop:
///   entry → header → body → (back to header) / exit
///
/// The body contains:
///   %fp_inv = fadd <flags> double %a, %b  (loop-invariant FP op)
///   %i_next = add i32 %i, 1               (loop-variant)
///
/// Returns (ctx, module, body_block_id).
fn build_fp_invariant_loop(flags: FastMathFlags) -> (Context, Module, BlockId) {
    let mut ctx = Context::new();
    let mut module = Module::new("test");
    let f64_ty = ctx.f64_ty;
    let i32_ty = ctx.i32_ty;

    let mut b = Builder::new(&mut ctx, &mut module);
    b.add_function(
        "f",
        i32_ty,
        vec![i32_ty, f64_ty, f64_ty],
        vec!["n".into(), "a".into(), "b".into()],
        false,
        Linkage::External,
    );

    let entry = b.add_block("entry");
    let header = b.add_block("header");
    let body = b.add_block("body");
    let exit_bb = b.add_block("exit");

    let c0 = b.const_int(i32_ty, 0);
    let c1 = b.const_int(i32_ty, 1);
    let n = b.get_arg(0);
    let a_fp = b.get_arg(1);
    let b_fp = b.get_arg(2);

    b.position_at_end(entry);
    b.build_br(header);

    b.position_at_end(header);
    let i_phi = b.build_phi("i", i32_ty, vec![(c0, entry)]);
    let cmp = b.build_icmp("cmp", llvm_ir::IntPredicate::Slt, i_phi, n);
    b.build_cond_br(cmp, body, exit_bb);

    b.position_at_end(body);
    // The FP op: loop-invariant (operands are function args), but only safe
    // to hoist when flags allow it.
    let _fp_inv = b.build_fadd("fp_inv", a_fp, b_fp);
    let _i_next = b.build_add("i_next", i_phi, c1);
    b.build_br(header);

    b.position_at_end(exit_bb);
    b.build_ret(c0);
    let _ = b;

    // Patch back-edge into phi.
    {
        let i_phi_iid = module.functions[0].value_names["i"];
        let i_next_iid = module.functions[0].value_names["i_next"];
        if let InstrKind::Phi { incoming, .. } =
            &mut module.functions[0].instructions[i_phi_iid.0 as usize].kind
        {
            incoming.push((ValueRef::Instruction(i_next_iid), body));
        }
    }

    // Overwrite the fadd flags to the desired value.
    {
        let fp_inv_iid = module.functions[0].value_names["fp_inv"];
        if let InstrKind::FAdd {
            flags: ref mut f, ..
        } = &mut module.functions[0].instructions[fp_inv_iid.0 as usize].kind
        {
            *f = flags;
        }
    }

    (ctx, module, body)
}

#[test]
fn licm_does_not_hoist_strict_fadd() {
    let (mut ctx, mut module, body) = build_fp_invariant_loop(flags_none());

    // Before LICM: body has fp_inv + i_next = 2 instrs.
    assert_eq!(
        module.functions[0].blocks[body.0 as usize].body.len(),
        2,
        "before LICM: body should have 2 instrs"
    );

    let mut pass = Licm;
    let _changed = pass.run_on_function(&mut ctx, &mut module.functions[0]);

    // fp_inv must NOT be hoisted: no FMF flags → strict FP semantics.
    let fp_inv_id = module.functions[0].value_names["fp_inv"];
    assert!(
        module.functions[0].blocks[body.0 as usize]
            .body
            .contains(&fp_inv_id),
        "%fp_inv must NOT be hoisted out of the loop (no fast-math flags)"
    );
}

// ---------------------------------------------------------------------------
// 4. LICM DOES hoist a fadd with reassoc
// ---------------------------------------------------------------------------

#[test]
fn licm_hoists_fadd_with_reassoc() {
    let (mut ctx, mut module, body) = build_fp_invariant_loop(flags_reassoc());

    let mut pass = Licm;
    let changed = pass.run_on_function(&mut ctx, &mut module.functions[0]);

    assert!(
        changed,
        "LICM must report a change when hoisting reassoc fadd"
    );

    // fp_inv must be hoisted out of the body.
    let fp_inv_id = module.functions[0].value_names["fp_inv"];
    assert!(
        !module.functions[0].blocks[body.0 as usize]
            .body
            .contains(&fp_inv_id),
        "%fp_inv must be hoisted out of the loop when reassoc is set"
    );
}

// ---------------------------------------------------------------------------
// 5. strictfp function attribute round-trips through printer + parser
// ---------------------------------------------------------------------------

#[test]
fn strictfp_function_attr_round_trips() {
    use llvm_ir::printer::Printer;
    use llvm_ir_parser::parser::parse;

    let mut ctx = Context::new();
    let mut module = Module::new("test");
    let f64_ty = ctx.f64_ty;
    {
        let mut b = Builder::new(&mut ctx, &mut module);
        b.add_function(
            "strict_fn",
            f64_ty,
            vec![f64_ty],
            vec!["x".into()],
            false,
            Linkage::External,
        );
        let entry = b.add_block("entry");
        b.position_at_end(entry);
        let x = b.get_arg(0);
        b.build_ret(x);
    }
    // Mark the function as strictfp.
    module.functions[0].strictfp = true;

    // Print to .ll text.
    let printer = Printer::new(&ctx);
    let ll = printer.print_module(&module);

    assert!(
        ll.contains("strictfp"),
        "printed .ll must contain 'strictfp' when function.strictfp = true; got:\n{}",
        ll
    );

    // Parse back.
    let (_ctx2, module2) = parse(&ll).expect("should re-parse successfully");

    assert!(
        module2.functions[0].strictfp,
        "parsed function must have strictfp=true"
    );
}

// ---------------------------------------------------------------------------
// 6. strictfp function-level override blocks FP CSE even when reassoc is set
// ---------------------------------------------------------------------------

#[test]
fn strictfp_blocks_fp_cse_entirely() {
    let (mut ctx, mut module) = make_f64_two_arg_fn();
    let f64_ty = ctx.f64_ty;
    let a = ValueRef::Argument(ArgId(0));
    let b = ValueRef::Argument(ArgId(1));

    // Both fadds carry reassoc (which normally makes them CSE-able).
    push_instr(
        &mut module,
        "x1",
        f64_ty,
        InstrKind::FAdd {
            flags: flags_reassoc(),
            lhs: a,
            rhs: b,
        },
    );
    let x2 = push_instr(
        &mut module,
        "x2",
        f64_ty,
        InstrKind::FAdd {
            flags: flags_reassoc(),
            lhs: a,
            rhs: b,
        },
    );
    set_ret(&ctx, &mut module, x2);

    // Mark the function as strictfp — this overrides per-instruction flags.
    module.functions[0].strictfp = true;

    assert_eq!(body_len(&module, BlockId(0)), 2, "before GVN: 2 fadds");

    let mut pass = Gvn;
    let changed = pass.run_on_function(&mut ctx, &mut module.functions[0]);

    // GVN must NOT fire because strictfp_mode=true overrides all FP CSE.
    assert!(
        !changed,
        "GVN must not CSE any FP ops in a strictfp function, even with reassoc"
    );
    assert_eq!(
        body_len(&module, BlockId(0)),
        2,
        "both fadds must remain in a strictfp function"
    );
}
