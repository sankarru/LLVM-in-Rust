/* Test: enum values used in switch/if
 * Expected exit code: 8
 * color = GREEN (1), switch returns 8 for GREEN
 */

typedef enum {
    RED   = 0,
    GREEN = 1,
    BLUE  = 2,
    ALPHA = 3
} Color;

int score_color(Color c) {
    switch (c) {
        case RED:   return 3;
        case GREEN: return 8;
        case BLUE:  return 5;
        case ALPHA: return 1;
        default:    return 0;
    }
}

int main(void) {
    Color c = GREEN;
    return score_color(c);
}
