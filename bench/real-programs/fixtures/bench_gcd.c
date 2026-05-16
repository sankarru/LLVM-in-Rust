/* Euclidean GCD(100003 + iter, 99991) for 50M iterations.
 * Returns last GCD result % 100 as exit code (matches bench_gcd.ll). */
int main(void) {
    long result = 0;
    long iter;
    for (iter = 0; iter < 50000000L; iter++) {
        long a = 100003L + iter;
        long b = 99991L;
        while (b != 0) { long t = b; b = a % b; a = t; }
        result = a;
    }
    return (int)(result % 100);
}
