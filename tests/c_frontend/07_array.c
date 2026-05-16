/* Test: array element access and summation
 * Expected exit code: 35
 * sum of {5, 10, 7, 8, 5} = 35
 */

int main(void) {
    int arr[5] = {5, 10, 7, 8, 5};
    int sum = 0;
    for (int i = 0; i < 5; i++) {
        sum = sum + arr[i];
    }
    return sum;
}
