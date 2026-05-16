/* Test: chain of add/sub/mul/div operations
 * Expected exit code: 7
 * Computation: ((3 + 5) * 4 - 2) / 3 = (32 - 2) / 3 = 30 / 3 = 10
 * Then: 10 * 10 - 93 = 7  -> mod 100 = 7
 */

int main(void) {
    int a = 3 + 5;      /* 8 */
    int b = a * 4;      /* 32 */
    int c = b - 2;      /* 30 */
    int d = c / 3;      /* 10 */
    int e = d * d;      /* 100 */
    int f = e - 93;     /* 7 */
    return f % 100;
}
