/* Link with arrays.f90. The first subscript varies fastest. */
extern void ScaleMatrix(int rows, int cols, float *x, float factor);
int main(void) {
    float x[6] = {1, 2, 3, 4, 5, 6};
    ScaleMatrix(2, 3, x, 2.0f);
    for (int i = 0; i < 6; ++i)
        if (x[i] != 2.0f * (i + 1)) return 1;
    return 0;
}
