program unused_example
implicit none
type :: unused_type
integer :: value
end type unused_type
integer, parameter :: unused_constant = 10
integer :: unused_variable, assigned_only, output
assigned_only=42
call useful_output(output)
print *,output
contains
subroutine useful_output(x)
integer,intent(out)::x
x=7
end subroutine useful_output
subroutine unused_procedure()
end subroutine unused_procedure
end program unused_example
