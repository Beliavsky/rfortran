program main
implicit none
real(kind=8),allocatable::x(:)
allocate(x(3))
call inspect(x)
deallocate(x)
allocate(x(0))
call inspect(x)
contains
subroutine inspect(a)
real(kind=8),intent(in)::a(:)
print *,huge(a)==huge(0._8),tiny(a)==tiny(0._8),kind(huge(a)),kind(tiny(a))
end subroutine
end
