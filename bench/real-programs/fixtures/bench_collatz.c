/* Collatz sequence: counts steps for starting values 1..500000.
 * Returns steps for start=500000, mod 100 = 51 (matches bench_collatz.ll). */
int main(void) {
    long sv3 = 0;
    long start;
    for (start = 1; start <= 500000L; start++) {
        long n = start, steps = 0;
        while (n != 1) {
            if (n % 2 == 0) n = n / 2;
            else n = 3 * n + 1;
            steps++;
        }
        sv3 = steps;
    }
    return (int)(sv3 % 100);
}
