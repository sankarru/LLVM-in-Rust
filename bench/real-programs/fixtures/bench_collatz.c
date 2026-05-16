/* Collatz sequence steps for all starting values 1..500000.
 * Returns steps % 100 as exit code (steps for n=500000).
 * Scalar-only: all variables are promoted to SSA by mem2reg. */
int main(void) {
    long result = 0;
    long start;
    for (start = 1; start <= 500000L; start++) {
        long n = start;
        long steps = 0;
        while (n != 1) {
            if (n % 2 == 0) n = n / 2;
            else n = 3 * n + 1;
            steps++;
        }
        result = steps;
    }
    return (int)(result % 100);
}
