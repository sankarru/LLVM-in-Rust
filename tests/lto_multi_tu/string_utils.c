// string_utils.c — string helpers.
// strlen_naive is never called from main.c (demonstrates dead-stripping).

int strlen_naive(const char *s) {
    int len = 0;
    while (*s++) len++;
    return len;
}

int char_at(const char *s, int idx) {
    return (int)s[idx];
}
