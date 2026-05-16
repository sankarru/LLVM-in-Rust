/* Test: static local variable incremented in a loop
 * Expected exit code: 10
 * Loop runs 10 times incrementing static counter, returns final value
 */

int increment_counter(void) {
    static int counter = 0;
    counter = counter + 1;
    return counter;
}

int main(void) {
    int result = 0;
    for (int i = 0; i < 10; i++) {
        result = increment_counter();
    }
    return result;
}
