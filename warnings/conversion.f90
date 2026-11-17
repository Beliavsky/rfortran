! Warning:    conversion
! Enabled by: -Weverything
! An implicit conversion that may lose information: REAL to INTEGER, a
! wider kind to a narrower one, or COMPLEX to REAL. An explicit conversion,
! such as nint(x) or real(x, kind=sp), shows that the loss is intended.
program conversion
use iso_fortran_env, only: sp => real32, dp => real64
implicit none
integer :: n
real(sp) :: s
real(dp) :: d
d = 2.75_dp
n = d                   ! expect: conversion
s = d                   ! expect: conversion
print *, n, s
n = nint(d)
s = real(d, kind=sp)
print *, n, s
end program conversion
