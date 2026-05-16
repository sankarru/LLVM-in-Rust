/* Test: function with multiple return paths
 * Expected exit code: 7
 * classify(15) hits the >10 branch and returns 7
 */

int classify(int n) {
    if (n < 0) {
        return 1;
    } else if (n == 0) {
        return 2;
    } else if (n < 10) {
        return 5;
    } else {
        return 7;
    }
}

int main(void) {
    return classify(15);
}
