; Benchmark: Collatz sequence
; Counts steps for all starting values 1..500000.
; Returns the last step-count % 100.
define i32 @main() {
entry:
  %start = alloca i64
  %n     = alloca i64
  %steps = alloca i64
  store i64 1, ptr %start
  br label %outer

outer:
  %startv    = load i64, ptr %start
  %outer_done = icmp sgt i64 %startv, 500000
  br i1 %outer_done, label %exit, label %outer_body

outer_body:
  store i64 %startv, ptr %n
  store i64 0, ptr %steps
  br label %collatz

collatz:
  %nv   = load i64, ptr %n
  %at1  = icmp eq i64 %nv, 1
  br i1 %at1, label %collatz_exit, label %collatz_step

collatz_step:
  %rem2    = srem i64 %nv, 2
  %is_even = icmp eq i64 %rem2, 0
  br i1 %is_even, label %even, label %odd

even:
  %half = sdiv i64 %nv, 2
  store i64 %half, ptr %n
  br label %count_step

odd:
  %triple = mul i64 %nv, 3
  %plus1  = add i64 %triple, 1
  store i64 %plus1, ptr %n
  br label %count_step

count_step:
  %sv  = load i64, ptr %steps
  %sv2 = add i64 %sv, 1
  store i64 %sv2, ptr %steps
  br label %collatz

collatz_exit:
  %startv2 = load i64, ptr %start
  %startv3 = add i64 %startv2, 1
  store i64 %startv3, ptr %start
  br label %outer

exit:
  %sv3   = load i64, ptr %steps
  %rem   = srem i64 %sv3, 100
  %trunc = trunc i64 %rem to i32
  ret i32 %trunc
}
