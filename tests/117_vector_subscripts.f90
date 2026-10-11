program vector_subscripts
implicit none
integer :: a(6), idx(3), i, b(-2:2), ix(-1:1), empty(0)
logical :: flags(3)
real :: x(3)
x = [10.0, 20.0, 30.0]
print *, x([1, 2, 1])
x([1, 2]) = x([2, 1])
print *, x
x([1, 2]) = -1.0
print *, x
a = [10, 20, 30, 40, 50, 60]
idx = [5, 1, 3]
print *, a(idx)
print *, a(idx + 0), a([(2*i-1, i=1,3)])
print *, a(idx(3:1:-1)), a([idx(1), idx(1)])
a(idx) = a(1:3)
print *, a
a = [2, 3, 1, 6, 4, 5]
a(a) = a
print *, a
a([2, 4, 6]) = [20, 40, 60]
print *, a
b = [1, 2, 3, 4, 5]
ix = [2, -2, 0]
print *, b(ix)
b(ix) = b(-2:0)
print *, b
flags = [.true., .false., .true.]
print *, flags([2, 3, 2]), all(flags([1, 3]))
flags([1, 3]) = .false.
print *, flags
print *, sum(a(idx)), size(a(idx)), lbound(a(idx)), ubound(a(idx))
print *, size(a(empty)), all(a(empty) == 0)
a(empty) = -9
print *, a
end program vector_subscripts
