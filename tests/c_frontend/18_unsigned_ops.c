/* Test: unsigned arithmetic and comparison
 * Expected exit code: 1
 * (unsigned)(-1) is a very large number > 100u, so result = 1
 */

int main(void) {
    unsigned int a = 0xFFFFFFFFu;  /* max unsigned 32-bit */
    unsigned int b = 100u;
    int result;
    if (a > b) {
        result = 1;
    } else {
        result = 0;
    }
    return result;
}
