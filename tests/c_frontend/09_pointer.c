/* Test: pointer arithmetic and dereference
 * Expected exit code: 37
 * arr[2] = 37, accessed via pointer
 */

int main(void) {
    int arr[5] = {10, 20, 37, 40, 50};
    int *p = arr;
    p = p + 2;
    return *p;
}
