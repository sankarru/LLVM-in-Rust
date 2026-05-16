/* Test: bitwise AND/OR/XOR/shift operations
 * Expected exit code: 93
 * Computation:
 *   a = 0b10110110 = 182
 *   b = 0b01101111 = 111
 *   c = a & b       = 0b00100110 = 38
 *   d = a | b       = 0b11111111 = 255
 *   e = a ^ b       = 0b11011001 = 217
 *   f = a >> 1      = 91
 *   g = b << 1      = 222
 *   result = (c + f) % 100 = (38 + 55) % 100 = 93
 *   Note: f = 182 >> 1 = 91, then c + f = 38 + 91 = 129, 129 % 100 = 29... let me recalc
 *   Actually: result = (c ^ f) % 100 = (38 ^ 91) = 125 % 100 = 25... use simpler values
 *   Recomputed with simpler path: see code
 */

int main(void) {
    int a = 0xF0;       /* 240 */
    int b = 0x0F;       /* 15  */
    int c = a | b;      /* 255 */
    int d = a & 0x3F;   /* 48  */
    int e = b ^ 0x06;   /* 9   */
    int f = d + e;      /* 57  */
    int g = c >> 1;     /* 127 */
    int result = (f + g) % 100; /* (57 + 127) % 100 = 184 % 100 = 84 */
    /* adjust: (48 + 9 + 36) % 100 = 93 */
    int h = 1 << 5;     /* 32 */
    int r = (d + e + h + 4) % 100; /* (48 + 9 + 32 + 4) % 100 = 93 */
    return r;
}
