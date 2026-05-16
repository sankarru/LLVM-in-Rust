/* Test: char comparison and arithmetic
 * Expected exit code: 26
 * 'z' - 'a' = 25, then +1 = 26
 */

int main(void) {
    char lo = 'a';
    char hi = 'z';
    int diff = (int)(hi - lo);  /* 25 */
    return diff + 1;             /* 26 */
}
