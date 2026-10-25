program main
implicit none
character(len=5) :: words(3) = [character(len=5) :: 'one', '', 'three']
integer :: i
do i=1,size(words)
   print '(a)', trimmed(words(i))
end do
contains
function trimmed(text) result(out)
character(len=*), intent(in) :: text
character(len=:), allocatable :: out
out = trim(text)
end function trimmed
end program main
