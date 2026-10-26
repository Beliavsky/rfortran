/* C owns the buffer. Fortran borrows it without copying or taking ownership. */
static double values[3] = {1, 2, 3};
void *buffer(void) { return values; }
int verify(void) {
    return values[0] == 11 && values[1] == 12 && values[2] == 13;
}
