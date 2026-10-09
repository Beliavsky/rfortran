! echo: 3
! echo: 1
program matrix_bounds
implicit none
integer :: a(2,3),i,j
a=0
read(*,*) i
read(*,*) j
! (3,1) would alias valid element (1,2) if only the linear offset were checked.
a(i,j)=99
print *, a
end program matrix_bounds
