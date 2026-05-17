; Adler-32 checksum loop: 4 phi nodes all carrying back-edge forward references.
; Exercises the parser's phi forward-ref staging for multiple simultaneous patches.
;
; Equivalent C (simplified):
;   uint32_t adler32(const uint8_t *buf, int len) {
;       uint32_t s1 = 1, s2 = 0;
;       for (int i = 0; i < len; i++) {
;           s1 = (s1 + buf[i]) % 65521;
;           s2 = (s2 + s1)     % 65521;
;       }
;       return (s2 << 16) | s1;
;   }
define i32 @adler32(ptr %buf, i32 %len) {
entry:
  %len64 = sext i32 %len to i64
  br label %loop

loop:
  %i   = phi i64   [ 0, %entry ], [ %i_next,  %body ]
  %s1  = phi i32   [ 1, %entry ], [ %s1_next, %body ]
  %s2  = phi i32   [ 0, %entry ], [ %s2_next, %body ]
  %cmp = icmp slt i64 %i, %len64
  br i1 %cmp, label %body, label %exit

body:
  %ptr_i  = getelementptr i8, ptr %buf, i64 %i
  %byte   = load i8, ptr %ptr_i
  %byte32 = zext i8 %byte to i32
  %s1_add = add i32 %s1, %byte32
  %s1_next = urem i32 %s1_add, 65521
  %s2_add  = add i32 %s2, %s1_next
  %s2_next = urem i32 %s2_add, 65521
  %i_next  = add i64 %i, 1
  br label %loop

exit:
  %s2_sh = shl i32 %s2, 16
  %result = or i32 %s2_sh, %s1
  ret i32 %result
}
