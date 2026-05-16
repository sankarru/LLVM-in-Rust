/* Test: multiple chained function calls
 * Expected exit code: 62
 * double(3) = 6, add_ten(6) = 16, triple(16) = 48, add_ten(48) = 58, add_ten(58)...
 * Actually: double(3)=6, add_ten(6)=16, triple(16)=48, double(48)=96, subtract(96,34)=62
 */

int double_it(int x) {
    return x * 2;
}

int add_ten(int x) {
    return x + 10;
}

int triple(int x) {
    return x * 3;
}

int subtract(int x, int y) {
    return x - y;
}

int main(void) {
    int a = double_it(3);       /* 6  */
    int b = add_ten(a);         /* 16 */
    int c = triple(b);          /* 48 */
    int d = double_it(c);       /* 96 */
    int e = subtract(d, 34);    /* 62 */
    return e;
}
