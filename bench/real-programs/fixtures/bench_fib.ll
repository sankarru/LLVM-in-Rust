; Benchmark: iterative Fibonacci
; Computes fib(30)=832040, repeated 20 million times.
; Returns fib(30) % 100 = 40.
define i32 @main() {
entry:
  %iter  = alloca i64
  %a     = alloca i64
  %b     = alloca i64
  %i     = alloca i64
  store i64 0, ptr %iter
  br label %outer

outer:
  %iterv = load i64, ptr %iter
  %done  = icmp sge i64 %iterv, 20000000
  br i1 %done, label %exit, label %outer_body

outer_body:
  store i64 0, ptr %a
  store i64 1, ptr %b
  store i64 2, ptr %i
  br label %inner

inner:
  %iv        = load i64, ptr %i
  %inner_done = icmp sgt i64 %iv, 30
  br i1 %inner_done, label %inner_exit, label %inner_body

inner_body:
  %av  = load i64, ptr %a
  %bv  = load i64, ptr %b
  %cv  = add i64 %av, %bv
  store i64 %bv, ptr %a
  store i64 %cv, ptr %b
  %iv2 = add i64 %iv, 1
  store i64 %iv2, ptr %i
  br label %inner

inner_exit:
  %iterv2 = load i64, ptr %iter
  %iterv3 = add i64 %iterv2, 1
  store i64 %iterv3, ptr %iter
  br label %outer

exit:
  %result = load i64, ptr %b
  %rem    = srem i64 %result, 100
  %trunc  = trunc i64 %rem to i32
  ret i32 %trunc
}
