/* Test: switch statement with multiple cases
 * Expected exit code: 3
 * input = 2, falls into case 2 which returns 3
 */

int classify_switch(int x) {
    switch (x) {
        case 0:
            return 10;
        case 1:
            return 1;
        case 2:
            return 3;
        case 3:
            return 9;
        default:
            return 0;
    }
}

int main(void) {
    return classify_switch(2);
}
