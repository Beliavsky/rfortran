/* Compile this file separately, then link with assumed_size.f90. */
extern double total(const double *x, int count);
extern void fill(double *x, int count);

int main(void) {
    double x[4] = {0, 0, 0, 99};
    fill(x, 3);
    fill(x, 0);
    return total(x, 3) == 3.0 && total(x, 0) == 0.0 && x[3] == 99
        ? 0 : 1;
}
