/* Test: ternary operator
 * Expected exit code: 99
 * 100 > 50 is true, so result = 99
 */

int main(void) {
    int a = 100;
    int b = 50;
    int result = (a > b) ? 99 : 1;
    return result;
}
