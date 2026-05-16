/* Test: global variable read in main
 * Expected exit code: 66
 */

int global_value = 66;

int main(void) {
    return global_value;
}
