// math_utils.c — small math helpers intended to be inlined by LTO.
// These functions are tiny so the LTO inliner should eliminate the call sites.

int add(int a, int b) {
    return a + b;
}

int square(int x) {
    return x * x;
}

int multiply(int a, int b) {
    return a * b;
}
