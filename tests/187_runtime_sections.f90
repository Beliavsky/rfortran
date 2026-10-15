program runtime_sections
implicit none
integer, allocatable :: a(:,:)
character(3), allocatable :: words(:,:)
integer :: n, step
n=3
allocate(a(2,n),words(2,n))
call fill(a(1,:))
call fill(a(2,:))
a(:,2:3)=a(:,1:2)+1
step=-1
if (any(a(1,3:1:step)/=[5,5,4])) stop 1
words='abc'
words(2,:)='def'
call check_words(words)
print '(a)', 'runtime sections ok'
contains
subroutine fill(v)
integer, intent(out) :: v(:)
v=4
end subroutine fill
subroutine check_words(v)
character(*), intent(in) :: v(:,:)
if (any(v(1,:)/='abc')) stop 2
if (any(v(2,:)/='def')) stop 3
end subroutine check_words
end program runtime_sections
