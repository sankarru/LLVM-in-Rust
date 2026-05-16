; Benchmark: Euclidean GCD
; Computes GCD(100003 + iter, 99991) for 50 million iterations.
; Returns last GCD result % 100.
define i32 @main() {
entry:
  %iter = alloca i64
  %a    = alloca i64
  %b    = alloca i64
  store i64 0, ptr %iter
  br label %loop

loop:
  %iterv = load i64, ptr %iter
  %done  = icmp sge i64 %iterv, 50000000
  br i1 %done, label %exit, label %loop_body

loop_body:
  %base = add i64 %iterv, 100003
  store i64 %base, ptr %a
  store i64 99991, ptr %b
  br label %gcd

gcd:
  %bv      = load i64, ptr %b
  %gcd_done = icmp eq i64 %bv, 0
  br i1 %gcd_done, label %gcd_exit, label %gcd_body

gcd_body:
  %av  = load i64, ptr %a
  %rem = srem i64 %av, %bv
  store i64 %bv, ptr %a
  store i64 %rem, ptr %b
  br label %gcd

gcd_exit:
  %iterv2 = load i64, ptr %iter
  %iterv3 = add i64 %iterv2, 1
  store i64 %iterv3, ptr %iter
  br label %loop

exit:
  %result = load i64, ptr %a
  %rem2   = srem i64 %result, 100
  %trunc  = trunc i64 %rem2 to i32
  ret i32 %trunc
}
