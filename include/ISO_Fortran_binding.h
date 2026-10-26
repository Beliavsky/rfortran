/* SPDX-License-Identifier: MIT
 * Copyright (c) 2026 Beliavsky
 * rfortran's numeric C descriptor ABI. Use this header with rfort objects.
 * Allocation-changing descriptors and other CFI routines are not yet supported.
 */
#ifndef RFORT_ISO_FORTRAN_BINDING_H
#define RFORT_ISO_FORTRAN_BINDING_H
#include <stddef.h>
#include <stdint.h>
#ifdef __cplusplus
extern "C" {
#endif
#define CFI_VERSION 1
#define CFI_MAX_RANK 15
typedef ptrdiff_t CFI_index_t;
typedef int8_t CFI_rank_t;
typedef int8_t CFI_attribute_t;
typedef int16_t CFI_type_t;
#define CFI_attribute_pointer 0
#define CFI_attribute_allocatable 1
#define CFI_attribute_other 2
#define CFI_type_Integer 1
#define CFI_type_Real 2
#define CFI_type_signed_char (CFI_type_Integer + (1 << 8))
#define CFI_type_short (CFI_type_Integer + (2 << 8))
#define CFI_type_int (CFI_type_Integer + ((int)sizeof(int) << 8))
#define CFI_type_long (CFI_type_Integer + ((int)sizeof(long) << 8))
#define CFI_type_long_long (CFI_type_Integer + ((int)sizeof(long long) << 8))
#define CFI_type_int8_t CFI_type_signed_char
#define CFI_type_int16_t CFI_type_short
#define CFI_type_int32_t (CFI_type_Integer + (4 << 8))
#define CFI_type_int64_t (CFI_type_Integer + (8 << 8))
#define CFI_type_size_t (CFI_type_Integer + ((int)sizeof(size_t) << 8))
#define CFI_type_intptr_t (CFI_type_Integer + ((int)sizeof(intptr_t) << 8))
#define CFI_type_ptrdiff_t (CFI_type_Integer + ((int)sizeof(ptrdiff_t) << 8))
#define CFI_type_float (CFI_type_Real + (4 << 8))
#define CFI_type_double (CFI_type_Real + (8 << 8))
#define CFI_SUCCESS 0
#define CFI_ERROR_BASE_ADDR_NULL 1
#define CFI_INVALID_ELEM_LEN 2
#define CFI_INVALID_RANK 3
#define CFI_INVALID_TYPE 4
#define CFI_INVALID_ATTRIBUTE 5
#define CFI_INVALID_EXTENT 6
#define CFI_INVALID_STRIDE 7
#define CFI_INVALID_DESCRIPTOR 8
#define CFI_ERROR_MEM_ALLOCATION 9
#define CFI_ERROR_OUT_OF_BOUNDS 10
typedef struct {
    CFI_index_t lower_bound, extent, sm; /* SM is a signed byte stride. */
} CFI_dim_t;
#define RFORT_CFI_PREFIX \
    void *base_addr; size_t elem_len; int version; \
    CFI_rank_t rank; CFI_attribute_t attribute; CFI_type_t type;
typedef struct {
    RFORT_CFI_PREFIX
#ifdef __cplusplus
    CFI_dim_t dim[1];
#else
    CFI_dim_t dim[];
#endif
} CFI_cdesc_t;
#define CFI_CDESC_T(r) struct { RFORT_CFI_PREFIX CFI_dim_t dim[(r) ? (r) : 1]; }
int CFI_establish(CFI_cdesc_t *, void *, CFI_attribute_t, CFI_type_t,
                  size_t, CFI_rank_t, const CFI_index_t []);
void *CFI_address(const CFI_cdesc_t *, const CFI_index_t []);
int CFI_is_contiguous(const CFI_cdesc_t *);
#ifdef __cplusplus
}
#endif
#endif
