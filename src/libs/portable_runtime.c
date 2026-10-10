/* SPDX-License-Identifier: MIT
 * Copyright (c) 2026 Beliavsky
 * See LICENSE-MIT in the repository root.
 * Fixed-argument C adapters keep variadic ABI details in the host C compiler.
 */
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <unistd.h>

int rfort_printf(const char *format, const char *value) {
    return printf(format, value);
}
int rfort_scanf(const char *format, void *value) {
    return scanf(format, value);
}
int rfort_sprintf(char *buffer, const char *format, int64_t value) {
    size_t n = strlen(format);
    char conversion = n ? format[n - 1] : 0;
    if (conversion == 'f') {
        double real;
        memcpy(&real, &value, sizeof real);
        return sprintf(buffer, format, real);
    }
    if (conversion == 's') return sprintf(buffer, format, (const char *)(intptr_t)value);
    return sprintf(buffer, format, (long long)value);
}
char *rfort_gcvt(double value, int digits, char *buffer) {
    snprintf(buffer, 64, "%.*g", digits, value);
    return buffer;
}
int rfort_write(int fd, const void *buffer, unsigned int size) {
    return (int)write(fd, buffer, size);
}
