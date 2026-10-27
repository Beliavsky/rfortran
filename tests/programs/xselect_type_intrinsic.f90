program select_type_intrinsic
use iso_fortran_env, only: real64
implicit none
integer :: i
real(real64) :: x
character(len=5) :: s
i=42
x=2.5_real64
s='hello'
call describe(i)
call describe(x)
call describe(s)
call describe(.true.)
call describe((1.0,2.0))
contains
subroutine describe(item)
class(*), intent(in) :: item
select type(value => item)
type is(integer)
   print *, 'integer', value
type is(real(real64))
   print *, 'real64', value
type is(character(len=*))
   print *, 'character', len(value), value
type is(logical)
   print *, 'logical', value
class default
   print *, 'other'
end select
end subroutine
end program
