; Loop using SSA phi nodes with back-edge forward references.
; The parser's staged_phi_patches / pending_phi_patches / resolve_phi_patches
; mechanism resolves %i_next and %acc_next after the full function body is seen.
define i64 @sum_n(i64 %n) {
entry:
  br label %loop
loop:
  %i   = phi i64 [ 0, %entry ], [ %i_next, %body ]
  %acc = phi i64 [ 0, %entry ], [ %acc_next, %body ]
  %done = icmp sge i64 %i, %n
  br i1 %done, label %exit, label %body
body:
  %acc_next = add i64 %acc, %i
  %i_next   = add i64 %i, 1
  br label %loop
exit:
  ret i64 %acc
}
