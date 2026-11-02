program c_interop
use iso_c_binding, only: c_int
implicit none
interface
   function c_abs(n) bind(c, name="abs") result(value)
      import c_int
      integer(c_int), value :: n
      integer(c_int) :: value
   end function c_abs
end interface
print *, c_abs(-42_c_int)
end program c_interop
