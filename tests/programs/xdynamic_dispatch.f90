module dynamic_counters
implicit none
type counter
   integer :: value=1
contains
   procedure :: get => counter_get
   procedure :: add => counter_add
   procedure :: report
end type
type,extends(counter) :: scaled_counter
   integer :: factor=2
contains
   procedure :: get => scaled_get
   procedure :: add => scaled_add
end type
type,extends(scaled_counter) :: inherited_counter
end type
contains
integer function counter_get(self) result(n)
class(counter),intent(in)::self
n=self%value
end function
integer function scaled_get(self) result(n)
class(scaled_counter),intent(in)::self
n=self%value*self%factor
end function
subroutine counter_add(self,amount)
class(counter),intent(inout)::self
integer,intent(in)::amount
self%value=self%value+amount
end subroutine
subroutine scaled_add(self,amount)
class(scaled_counter),intent(inout)::self
integer,intent(in)::amount
self%value=self%value+amount*self%factor
end subroutine
subroutine report(self)
class(counter),intent(in)::self
print *,self%get()
end subroutine
subroutine show(x)
class(counter),intent(inout)::x
print *,x%get()
call x%add(amount=2)
print *,x%get()
end subroutine
end module
program test_dynamic_dispatch
use dynamic_counters
implicit none
type(counter)::a
type(scaled_counter)::b
type(inherited_counter)::c
b%value=3
c%value=4
c%factor=3
call show(a)
call show(b)
call show(c)
call b%report()
print *,a%value,b%value,c%value
end program
