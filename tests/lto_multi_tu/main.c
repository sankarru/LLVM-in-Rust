// main.c — uses math_utils.c helpers; does NOT call strlen_naive from string_utils.c.
// After LTO, cross-module inlining should eliminate the call to add() and square(),
// and dead-stripping should remove strlen_naive (never called).

int add(int a, int b);
int square(int x);
int char_at(const char *s, int idx);

int main(void) {
    // These should be inlined by LTO.
    int a = add(3, 4);     // 7
    int b = square(3);     // 9
    return a + b - 16;     // 7 + 9 - 16 = 0 (success)
}
