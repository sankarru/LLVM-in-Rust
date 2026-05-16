/* Test: recursion (Fibonacci)
 * Expected exit code: 55
 * fibonacci(10) = 55
 */

int fibonacci(int n) {
    if (n <= 1) {
        return n;
    }
    return fibonacci(n - 1) + fibonacci(n - 2);
}

int main(void) {
    return fibonacci(10) % 100;
}
