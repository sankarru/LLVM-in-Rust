/* Test: nested for loops computing a sum
 * Expected exit code: 25
 * Outer loop i: 1..5, inner loop j: 1..i, sum += 1
 * Counts: i=1:1, i=2:2, i=3:3, i=4:4, i=5:5 => total = 15
 * Then multiply by outer i at end for last row: actually just count pairs
 * sum = 1+2+3+4+5 = 15, then add 10 = 25
 */

int main(void) {
    int sum = 0;
    for (int i = 1; i <= 5; i++) {
        for (int j = 1; j <= i; j++) {
            sum = sum + 1;
        }
    }
    /* sum == 15 here */
    sum = sum + 10;
    return sum % 100;
}
