! Compile pointer_buffer.c separately, then link it with this program using rfort.
program pointer_buffer_example
use iso_c_binding, only: c_ptr, c_double, c_associated, c_f_pointer
implicit none
interface
   function buffer() bind(c) result(p)
      import c_ptr
      type(c_ptr) :: p
   end function
   function verify() bind(c) result(i)
      integer :: i
   end function
end interface
type(c_ptr) :: p
real(c_double), pointer :: a(:)
p = buffer()
if (.not. c_associated(p)) stop 1
call c_f_pointer(p, a, [3])
print *, size(a), a
a = a + 10.0_c_double
print *, verify()
end program
