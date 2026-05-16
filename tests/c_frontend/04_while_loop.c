/* Test: while loop accumulation
 * Expected exit code: 55
 * sum of 1..10 = 55
 */

int main(void) {
    int i = 1;
    int sum = 0;
    while (i <= 10) {
        sum = sum + i;
        i = i + 1;
    }
    return sum;
}
