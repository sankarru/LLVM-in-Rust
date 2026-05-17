; Hand-written LLVM IR representing SQLite's strcmp-style comparison loop.
; A pointer-advancing loop with two pointer phi nodes carrying back-edge
; forward references, plus a condition phi for the final result.
;
; Mirrors the structure of SQLite's sqlite3StrICmp / memcmp helpers.

define i32 @sqlite_strcmp(ptr %a, ptr %b) {
entry:
  br label %loop

loop:
  %pa   = phi ptr [ %a, %entry ], [ %pa_next, %cont ]
  %pb   = phi ptr [ %b, %entry ], [ %pb_next, %cont ]
  %ca   = load i8, ptr %pa
  %cb   = load i8, ptr %pb
  %ca32 = zext i8 %ca to i32
  %cb32 = zext i8 %cb to i32
  %diff = sub i32 %ca32, %cb32
  %end  = icmp eq i8 %ca, 0
  br i1 %end, label %exit, label %check

check:
  %neq = icmp ne i32 %diff, 0
  br i1 %neq, label %exit, label %cont

cont:
  %pa_next = getelementptr i8, ptr %pa, i64 1
  %pb_next = getelementptr i8, ptr %pb, i64 1
  br label %loop

exit:
  ret i32 %diff
}
