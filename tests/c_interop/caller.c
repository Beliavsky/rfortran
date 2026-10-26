/* C caller for scalar.f90. Exit zero means both calls passed. */
extern int AddMixed(int, float);
extern void bump_values(int *, double *);
int main(void) {
    int x = 4;
    double y;
    if (AddMixed(x, 2.5f) != 7 || x != 4) return 1;
    bump_values(&x, &y);
    return x == 6 && y == 3.5 ? 0 : 2;
}
