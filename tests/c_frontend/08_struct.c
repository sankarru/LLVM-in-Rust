/* Test: struct with two int fields
 * Expected exit code: 75
 * point.x + point.y = 30 + 45 = 75
 */

struct Point {
    int x;
    int y;
};

int main(void) {
    struct Point p;
    p.x = 30;
    p.y = 45;
    return p.x + p.y;
}
