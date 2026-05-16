/* Test: if/else with comparisons
 * Expected exit code: 1
 * 10 > 5 is true, so returns 1
 */

int main(void) {
    int x = 10;
    int y = 5;
    if (x > y) {
        return 1;
    } else {
        return 0;
    }
}
