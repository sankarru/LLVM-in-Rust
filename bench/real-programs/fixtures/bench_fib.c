/* Iterative Fibonacci: compute fib(30) = 832040, repeated 20M times.
 * Returns fib(30) % 100 = 40 as exit code.
 * Scalar-only: all variables are promoted to SSA by mem2reg. */
int main(void) {
    long result = 0;
    long iter;
    for (iter = 0; iter < 20000000L; iter++) {
        long a = 0, b = 1, c;
        int i;
        for (i = 2; i <= 30; i++) { c = a + b; a = b; b = c; }
        result = b;
    }
    return (int)(result % 100); /* fib(30)=832040, 832040%100=40 */
}
