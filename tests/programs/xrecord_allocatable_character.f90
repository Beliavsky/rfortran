program test
implicit none
type :: row
character(len=5), allocatable :: names(:)
real, allocatable :: values(:,:)
end type
type(row) :: a(2)
print *, allocated(a(1)%names), allocated(a(2)%names)
allocate(a(1)%names(2), a(1)%values(2,2))
a(1)%names = ["alpha", "beta "]
a(1)%values = reshape([1.,2.,3.,4.], [2,2])
a(2) = a(1)
a(1)%names(1) = "other"
a(1)%values(1,1) = 9.
print '(a)', a(2)%names
print *, sum(a(2)%values), size(a(2)%names)
deallocate(a(1)%names)
print *, allocated(a(1)%names), allocated(a(2)%names)
end program
