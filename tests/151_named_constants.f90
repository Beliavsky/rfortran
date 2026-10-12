program test_named_constants
implicit none
logical, parameter :: a = .false., b = .not.a
logical, parameter :: c = (a .or. b) .and. (a .neqv. b)
logical, parameter :: flags(*) = [a, b, c]
logical, parameter :: any_set = any(flags), all_set = all(flags)
integer(kind=1), parameter :: i1 = 12
integer(kind=2), parameter :: i2 = 1234
integer(kind=4), parameter :: i4 = 123456
integer(kind=8), parameter :: i8 = 10000000000_8
integer(kind=8), parameter :: big(2) = [i8, i8+1_8]
real, parameter :: r = 1.25
double precision, parameter :: d = 2.5d0
real(kind=8), parameter :: d2 = d * 2
real, parameter :: rr(2) = [r, r*2]
double precision, parameter :: dd(2) = [d, d2]
character(len=*), parameter :: text = 'ab' // achar(67)
character(2), parameter :: words(2) = ['ab', 'cd']
print *, a, b, c, any_set, all_set
print *, flags
print *, kind(i1), kind(i2), kind(i4), kind(i8)
print *, i1, i2, i4, i8
print *, big
print *, r, d, d2
print *, rr, dd
print '(a)', text
print '(*(1x,a))', words
end program test_named_constants
