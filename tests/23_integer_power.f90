! echo: 3
! echo: 34
program test_integer_power
    implicit none
    integer :: base, exponent
    read(*,*) base
    read(*,*) exponent
    print *, "constant:", 5**2, 3**34
    print *, "runtime:", base**exponent
    print *, "zero and one:", base**0, base**1
    print *, "negative:", base**(-2), (-1)**(-3), (-1)**(-4)
    print *, "negative unit:", (base-4)**(-exponent), (base-4)**(-exponent-1)
    print *, "sign:", (-base)**3, (-base)**4
    print *, "real:", 5.0**2, 5**2.0, 5.0**0, 5**1.0
end program test_integer_power
