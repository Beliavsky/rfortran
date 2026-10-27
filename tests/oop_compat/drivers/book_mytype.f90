! rfortran test driver; upstream module is unchanged.
program test_mytype
use mytype_module
implicit none
type(mytype) :: x
x%myvalue = [1.0, 2.0, 3.0, 4.0]
call x%write()
call x%reset()
call x%write()
end program
