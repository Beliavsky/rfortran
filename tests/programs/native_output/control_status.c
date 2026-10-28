/* A printf error changes the exit code; this also links no rfortran code. */
#include <stdio.h>
int main(void) {
    int written = printf("42\n");
    return written == 3 ? 0 : 17;
}
