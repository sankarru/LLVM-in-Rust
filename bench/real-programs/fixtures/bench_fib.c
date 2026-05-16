/* Iterative Fibonacci: fib(30) = 832040, repeated 20M times.
 * Returns fib(30) % 100 = 40 as exit code (matches bench_fib.ll).
 *
 * volatile prevents clang -O2 from constant-folding the entire loop. */
int main(void) {
    volatile long iters = 20000000L;
    long b = 1;
    long iter;
    for (iter = 0; iter < iters; iter++) {
        long a = 0, b_ = 1, c;
        long i;
        for (i = 2; i <= 30; i++) { c = a + b_; a = b_; b_ = c; }
        b = b_;
    }
    return (int)(b % 100);
}
