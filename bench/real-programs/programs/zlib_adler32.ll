; Hand-written LLVM IR representing zlib's adler32 inner loop pattern.
; Two accumulator phi nodes (s1, s2) plus a pointer-offset phi and a counter,
; all with back-edge forward references resolved by the parser's phi-patch pass.
;
; Mirrors the structure of zlib adler32.c update loop.

define i32 @adler32(i32 %adler, ptr %buf, i32 %len) {
entry:
  %s1_init = and i32 %adler, 65535
  %s2_init = lshr i32 %adler, 16
  %len64   = sext i32 %len to i64
  br label %loop

loop:
  %i    = phi i64 [ 0, %entry ],    [ %i_next,   %body ]
  %s1   = phi i32 [ %s1_init, %entry ], [ %s1_next, %body ]
  %s2   = phi i32 [ %s2_init, %entry ], [ %s2_next, %body ]
  %cmp  = icmp ult i64 %i, %len64
  br i1 %cmp, label %body, label %exit

body:
  %gep    = getelementptr i8, ptr %buf, i64 %i
  %byte   = load i8, ptr %gep
  %byte32 = zext i8 %byte to i32
  %s1_add = add i32 %s1, %byte32
  %s1_next = urem i32 %s1_add, 65521
  %s2_add  = add i32 %s2, %s1_next
  %s2_next = urem i32 %s2_add, 65521
  %i_next  = add i64 %i, 1
  br label %loop

exit:
  %s2_sh  = shl i32 %s2, 16
  %result = or i32 %s2_sh, %s1
  ret i32 %result
}
