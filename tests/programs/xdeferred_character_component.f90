program main
implicit none
type table
character(:),allocatable::names(:)
end type
type(table)::t
t%names=['one','two']
call show(t)
t%names=['longer']
call show(t)
contains
subroutine show(t)
type(table),intent(in)::t
print *,size(t%names),len(t%names)
print '(a)',t%names
end subroutine
end program
