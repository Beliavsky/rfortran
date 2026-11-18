! Warning:    section-bounds
! Enabled by: -Wextra
! An ASSOCIATE selector or pointer target that is a section of a whole array,
! such as a(:), when the array's lower bound is not 1. A section's lower
! bounds are 1, so the associate name or pointer renumbers the elements; the
! array itself keeps its bounds. A section of part of an array, and a pointer
! assignment that gives the bounds, are not reported.
program section_bounds
implicit none
real, target :: a(0:4)
real, pointer :: p(:)
integer :: i
a = [(real(i), i = 0, 4)]
associate (x => a(:))                    ! expect: section-bounds
  print *, lbound(x), size(x)
end associate
associate (x => a)
  print *, lbound(x), size(x)
end associate
associate (x => a(1:3))
  print *, lbound(x), size(x)
end associate
p => a(:)                                ! expect: section-bounds
print *, lbound(p), p(1)
p(0:) => a(:)
print *, lbound(p), p(1)
p => a
print *, lbound(p), p(1)
end program section_bounds
