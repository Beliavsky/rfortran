/* Compile with -I include, then link with descriptors.f90 using rfort. */
#include <ISO_Fortran_binding.h>
extern void OffsetArray(CFI_cdesc_t *x, double delta);

int main(void) {
    double x[8] = {1, 2, 3, 4, 5, 6, 7, 8};
    CFI_CDESC_T(2) storage;
    CFI_cdesc_t *d = (CFI_cdesc_t *)&storage;
    CFI_index_t shape[2] = {2, 2};
    if (CFI_establish(d, x + 6, CFI_attribute_other, CFI_type_double,
                      sizeof(double), 2, shape) != CFI_SUCCESS) return 1;
    d->dim[0].sm = -2 * (CFI_index_t)sizeof(double);
    d->dim[1].sm = -4 * (CFI_index_t)sizeof(double);
    if (CFI_is_contiguous(d)) return 2;
    OffsetArray(d, 0.5);
    for (int i = 0; i < 8; ++i)
        if (x[i] != i + 1 + (i % 2 == 0 ? 0.5 : 0.0)) return 3;
    return 0;
}
