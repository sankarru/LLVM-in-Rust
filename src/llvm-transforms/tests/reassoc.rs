//! Tests for the FMF-aware reassociation pass (`ReassocPass`) and
//! FMF-exploiting FP constant folding (`try_fold_fp`).

use llvm_ir::{
    ArgId, Builder, ConstantData, Context, FastMathFlags, InstrId, InstrKind, Instruction, Linkage,
    Module, TypeId, ValueRef,
};
use llvm_transforms::{constant_fold::try_fold_fp, pass::FunctionPass, reassoc::ReassocPass};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build a minimal f64 function `f(f64 %x) -> f64` with an empty entry block.
fn make_f64_fn() -> (Context, Module, TypeId) {
    let mut ctx = Context::new();
    let mut module = Module::new("test");
    let f64_ty = ctx.f64_ty;
    {
        let mut b = Builder::new(&mut ctx, &mut module);
        b.add_function(
            "f",
            f64_ty,
            vec![f64_ty],
            vec!["x".into()],
            false,
            Linkage::External,
        );
        let entry = b.add_block("entry");
        b.position_at_end(entry);
        // No instructions yet — we'll add them manually.
    }
    (ctx, module, f64_ty)
}

/// Count body (non-terminator) instructions in block 0 of function 0.
fn body_len(module: &Module) -> usize {
    module.functions[0].blocks[0].body.len()
}

/// Helper: append an instruction to block 0 of function 0, return its InstrId.
fn push_instr(
    ctx: &Context,
    module: &mut Module,
    name: &str,
    ty: TypeId,
    kind: InstrKind,
) -> ValueRef {
    let f = &mut module.functions[0];
    let iid = f.alloc_instr(Instruction::new(Some(name.into()), ty, kind));
    f.blocks[0].body.push(iid);
    ValueRef::Instruction(iid)
}

/// Helper: set the terminator (ret) for block 0 of function 0.
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

fn flags_nsz() -> FastMathFlags {
    FastMathFlags {
        nsz: true,
        ..Default::default()
    }
}
fn flags_nnan() -> FastMathFlags {
    FastMathFlags {
        nnan: true,
        ..Default::default()
    }
}
fn flags_nnan_ninf() -> FastMathFlags {
    FastMathFlags {
        nnan: true,
        ninf: true,
        ..Default::default()
    }
}
fn flags_arcp() -> FastMathFlags {
    FastMathFlags {
        arcp: true,
        ..Default::default()
    }
}
fn flags_reassoc() -> FastMathFlags {
    FastMathFlags {
        reassoc: true,
        nnan: true,
        ..Default::default()
    }
}
fn flags_fast() -> FastMathFlags {
    FastMathFlags {
        fast: true,
        ..Default::default()
    }
}

// ---------------------------------------------------------------------------
// 1. nsz: x + 0.0  →  x
// ---------------------------------------------------------------------------

#[test]
fn nsz_fadd_zero_elim() {
    let (mut ctx, mut module, f64_ty) = make_f64_fn();
    let x = ValueRef::Argument(ArgId(0));
    let zero = ValueRef::Constant(ctx.const_float(f64_ty, 0f64.to_bits()));
    let r = push_instr(
        &ctx,
        &mut module,
        "r",
        f64_ty,
        InstrKind::FAdd {
            flags: flags_nsz(),
            lhs: x,
            rhs: zero,
        },
    );
    set_ret(&ctx, &mut module, r);

    assert_eq!(body_len(&module), 1, "one FAdd before pass");

    let mut pass = ReassocPass;
    let changed = pass.run_on_function(&mut ctx, &mut module.functions[0]);
    assert!(changed, "pass should report a change");
    assert_eq!(body_len(&module), 0, "FAdd should be eliminated");

    let f = &module.functions[0];
    let tid = f.blocks[0].terminator.unwrap();
    if let InstrKind::Ret { val: Some(v) } = &f.instr(tid).kind {
        assert_eq!(*v, ValueRef::Argument(ArgId(0)));
    } else {
        panic!("expected ret with argument");
    }
}

// ---------------------------------------------------------------------------
// 2. nsz: x - 0.0  →  x
// ---------------------------------------------------------------------------

#[test]
fn nsz_fsub_zero_elim() {
    let (mut ctx, mut module, f64_ty) = make_f64_fn();
    let x = ValueRef::Argument(ArgId(0));
    let zero = ValueRef::Constant(ctx.const_float(f64_ty, 0f64.to_bits()));
    let r = push_instr(
        &ctx,
        &mut module,
        "r",
        f64_ty,
        InstrKind::FSub {
            flags: flags_nsz(),
            lhs: x,
            rhs: zero,
        },
    );
    set_ret(&ctx, &mut module, r);

    let mut pass = ReassocPass;
    let changed = pass.run_on_function(&mut ctx, &mut module.functions[0]);
    assert!(changed);
    assert_eq!(body_len(&module), 0);
}

// ---------------------------------------------------------------------------
// 3. nnan: x * 1.0  →  x
// ---------------------------------------------------------------------------

#[test]
fn nnan_fmul_one_elim() {
    let (mut ctx, mut module, f64_ty) = make_f64_fn();
    let x = ValueRef::Argument(ArgId(0));
    let one = ValueRef::Constant(ctx.const_float(f64_ty, 1f64.to_bits()));
    let r = push_instr(
        &ctx,
        &mut module,
        "r",
        f64_ty,
        InstrKind::FMul {
            flags: flags_nnan(),
            lhs: x,
            rhs: one,
        },
    );
    set_ret(&ctx, &mut module, r);

    let mut pass = ReassocPass;
    let changed = pass.run_on_function(&mut ctx, &mut module.functions[0]);
    assert!(changed);
    assert_eq!(body_len(&module), 0);
}

// ---------------------------------------------------------------------------
// 4. arcp: try_fold_fp folds two FP constants under FDiv
// ---------------------------------------------------------------------------

#[test]
fn arcp_fdiv_to_fmul() {
    let mut ctx = Context::new();
    let f64_ty = ctx.f64_ty;
    let six = ValueRef::Constant(ctx.const_float(f64_ty, 6f64.to_bits()));
    let two = ValueRef::Constant(ctx.const_float(f64_ty, 2f64.to_bits()));
    let kind = InstrKind::FDiv {
        flags: flags_arcp(),
        lhs: six,
        rhs: two,
    };
    // 6.0 / 2.0 = 3.0
    let result = try_fold_fp(&mut ctx, &kind).expect("should fold two FP constants");
    match ctx.get_const(result) {
        ConstantData::Float { bits, .. } => {
            assert_eq!(f64::from_bits(*bits), 3.0f64);
        }
        other => panic!("expected Float constant, got {:?}", other),
    }
}

// ---------------------------------------------------------------------------
// 5. nnan + ninf: x * 0.0  →  0.0
// ---------------------------------------------------------------------------

#[test]
fn nnan_ninf_fmul_zero() {
    let (mut ctx, mut module, f64_ty) = make_f64_fn();
    let x = ValueRef::Argument(ArgId(0));
    let zero = ValueRef::Constant(ctx.const_float(f64_ty, 0f64.to_bits()));
    let r = push_instr(
        &ctx,
        &mut module,
        "r",
        f64_ty,
        InstrKind::FMul {
            flags: flags_nnan_ninf(),
            lhs: x,
            rhs: zero,
        },
    );
    set_ret(&ctx, &mut module, r);

    let mut pass = ReassocPass;
    let changed = pass.run_on_function(&mut ctx, &mut module.functions[0]);
    assert!(changed, "x * 0.0 should simplify with nnan+ninf");
    assert_eq!(body_len(&module), 0);

    // The ret should now reference a 0.0 constant.
    let f = &module.functions[0];
    let tid = f.blocks[0].terminator.unwrap();
    if let InstrKind::Ret {
        val: Some(ValueRef::Constant(cid)),
    } = &f.instr(tid).kind
    {
        match ctx.get_const(*cid) {
            ConstantData::Float { bits, .. } => assert_eq!(f64::from_bits(*bits), 0.0),
            other => panic!("expected 0.0 float, got {:?}", other),
        }
    } else {
        panic!("expected ret with constant 0.0");
    }
}

// ---------------------------------------------------------------------------
// 6. reassoc: (x + 1.0) + 2.0  →  x + 3.0
// ---------------------------------------------------------------------------

#[test]
fn reassoc_const_chain() {
    let (mut ctx, mut module, f64_ty) = make_f64_fn();
    let x = ValueRef::Argument(ArgId(0));
    let c1 = ValueRef::Constant(ctx.const_float(f64_ty, 1f64.to_bits()));
    let c2 = ValueRef::Constant(ctx.const_float(f64_ty, 2f64.to_bits()));

    // inner = fadd reassoc double %x, 1.0
    let inner = push_instr(
        &ctx,
        &mut module,
        "inner",
        f64_ty,
        InstrKind::FAdd {
            flags: flags_reassoc(),
            lhs: x,
            rhs: c1,
        },
    );

    // outer = fadd reassoc double %inner, 2.0
    let outer = push_instr(
        &ctx,
        &mut module,
        "outer",
        f64_ty,
        InstrKind::FAdd {
            flags: flags_reassoc(),
            lhs: inner,
            rhs: c2,
        },
    );

    set_ret(&ctx, &mut module, outer);

    assert_eq!(body_len(&module), 2, "two FAdds before pass");

    let mut pass = ReassocPass;
    let changed = pass.run_on_function(&mut ctx, &mut module.functions[0]);
    assert!(changed, "reassoc should combine constants");

    // After: look for a remaining FAdd whose rhs is 3.0.
    let f = &module.functions[0];
    let found = f.blocks[0].body.iter().any(|&iid| {
        if let InstrKind::FAdd {
            rhs: ValueRef::Constant(cid),
            ..
        } = &f.instr(iid).kind
        {
            if let ConstantData::Float { bits, .. } = ctx.get_const(*cid) {
                return f64::from_bits(*bits) == 3.0;
            }
        }
        false
    });
    assert!(found, "expected to find FAdd with rhs=3.0 after reassoc");
}

// ---------------------------------------------------------------------------
// 7. No fold without the relevant flag (nsz absent for x + 0.0)
// ---------------------------------------------------------------------------

#[test]
fn no_fold_without_flag() {
    let (mut ctx, mut module, f64_ty) = make_f64_fn();
    let x = ValueRef::Argument(ArgId(0));
    let zero = ValueRef::Constant(ctx.const_float(f64_ty, 0f64.to_bits()));
    let r = push_instr(
        &ctx,
        &mut module,
        "r",
        f64_ty,
        InstrKind::FAdd {
            flags: FastMathFlags::default(), // no nsz
            lhs: x,
            rhs: zero,
        },
    );
    set_ret(&ctx, &mut module, r);

    let mut pass = ReassocPass;
    let changed = pass.run_on_function(&mut ctx, &mut module.functions[0]);
    assert!(!changed, "should NOT simplify without nsz flag");
    assert_eq!(body_len(&module), 1, "FAdd must remain");
}

// ---------------------------------------------------------------------------
// 8. No fold: nnan absent for x * 1.0
// ---------------------------------------------------------------------------

#[test]
fn no_fold_strict_zero() {
    let (mut ctx, mut module, f64_ty) = make_f64_fn();
    let x = ValueRef::Argument(ArgId(0));
    let one = ValueRef::Constant(ctx.const_float(f64_ty, 1f64.to_bits()));
    let r = push_instr(
        &ctx,
        &mut module,
        "r",
        f64_ty,
        InstrKind::FMul {
            flags: FastMathFlags::default(), // no nnan
            lhs: x,
            rhs: one,
        },
    );
    set_ret(&ctx, &mut module, r);

    let mut pass = ReassocPass;
    let changed = pass.run_on_function(&mut ctx, &mut module.functions[0]);
    assert!(!changed, "should NOT simplify without nnan");
    assert_eq!(body_len(&module), 1);
}

// ---------------------------------------------------------------------------
// 9. fast flag enables all applicable folds
// ---------------------------------------------------------------------------

#[test]
fn fast_flag_enables_all() {
    let (mut ctx, mut module, f64_ty) = make_f64_fn();
    let x = ValueRef::Argument(ArgId(0));
    let zero = ValueRef::Constant(ctx.const_float(f64_ty, 0f64.to_bits()));
    let r = push_instr(
        &ctx,
        &mut module,
        "r",
        f64_ty,
        InstrKind::FAdd {
            flags: flags_fast(),
            lhs: x,
            rhs: zero,
        },
    );
    set_ret(&ctx, &mut module, r);

    let mut pass = ReassocPass;
    let changed = pass.run_on_function(&mut ctx, &mut module.functions[0]);
    assert!(changed, "fast flag should enable nsz-style folding");
    assert_eq!(body_len(&module), 0);
}

// ---------------------------------------------------------------------------
// 10. x / 1.0  →  x  when nnan
// ---------------------------------------------------------------------------

#[test]
fn fdiv_one_elim() {
    let (mut ctx, mut module, f64_ty) = make_f64_fn();
    let x = ValueRef::Argument(ArgId(0));
    let one = ValueRef::Constant(ctx.const_float(f64_ty, 1f64.to_bits()));
    let r = push_instr(
        &ctx,
        &mut module,
        "r",
        f64_ty,
        InstrKind::FDiv {
            flags: flags_nnan(),
            lhs: x,
            rhs: one,
        },
    );
    set_ret(&ctx, &mut module, r);

    let mut pass = ReassocPass;
    let changed = pass.run_on_function(&mut ctx, &mut module.functions[0]);
    assert!(changed, "x / 1.0 should simplify with nnan");
    assert_eq!(body_len(&module), 0);
}

// ---------------------------------------------------------------------------
// 11. try_fold_fp: two FP constants FAdd
// ---------------------------------------------------------------------------

#[test]
fn fold_fp_two_constants_fadd() {
    let mut ctx = Context::new();
    let f64_ty = ctx.f64_ty;
    let two = ValueRef::Constant(ctx.const_float(f64_ty, 2f64.to_bits()));
    let three = ValueRef::Constant(ctx.const_float(f64_ty, 3f64.to_bits()));
    let kind = InstrKind::FAdd {
        flags: FastMathFlags::default(),
        lhs: two,
        rhs: three,
    };
    let result = try_fold_fp(&mut ctx, &kind).expect("two consts should fold");
    match ctx.get_const(result) {
        ConstantData::Float { bits, .. } => assert_eq!(f64::from_bits(*bits), 5.0f64),
        other => panic!("expected Float, got {:?}", other),
    }
}

// ---------------------------------------------------------------------------
// 12. try_fold_fp: nnan suppresses NaN result
// ---------------------------------------------------------------------------

#[test]
fn fold_fp_nnan_suppresses_nan() {
    // 0.0 / 0.0 = NaN; with nnan, try_fold_fp should return None.
    let mut ctx = Context::new();
    let f64_ty = ctx.f64_ty;
    let zero = ValueRef::Constant(ctx.const_float(f64_ty, 0f64.to_bits()));
    let kind = InstrKind::FDiv {
        flags: FastMathFlags {
            nnan: true,
            ..Default::default()
        },
        lhs: zero,
        rhs: zero,
    };
    let result = try_fold_fp(&mut ctx, &kind);
    assert!(result.is_none(), "nnan should suppress NaN fold");
}

// ---------------------------------------------------------------------------
// 13. nsz: 0.0 + x  →  x  (commutative)
// ---------------------------------------------------------------------------

#[test]
fn nsz_fadd_zero_lhs_elim() {
    let (mut ctx, mut module, f64_ty) = make_f64_fn();
    let x = ValueRef::Argument(ArgId(0));
    let zero = ValueRef::Constant(ctx.const_float(f64_ty, 0f64.to_bits()));
    let r = push_instr(
        &ctx,
        &mut module,
        "r",
        f64_ty,
        InstrKind::FAdd {
            flags: flags_nsz(),
            lhs: zero,
            rhs: x,
        },
    );
    set_ret(&ctx, &mut module, r);

    let mut pass = ReassocPass;
    let changed = pass.run_on_function(&mut ctx, &mut module.functions[0]);
    assert!(changed, "0.0 + x should simplify with nsz");
    assert_eq!(body_len(&module), 0);
}
