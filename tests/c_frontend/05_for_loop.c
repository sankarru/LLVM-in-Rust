/* Test: for loop, factorial
 * Expected exit code: 20
 * factorial(5) = 120, 120 % 100 = 20
 */

int main(void) {
    int result = 1;
    for (int i = 1; i <= 5; i++) {
        result = result * i;
    }
    return result % 100;
}
