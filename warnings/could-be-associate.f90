! Warning:    could-be-associate
! Enabled by: -Wextra
! A POINTER that is associated once, with a variable or a part of one, and
! then only used for it could be an ASSOCIATE name, which cannot be
! associated again or left dangling, and whose selector does not need
! TARGET. A pointer associated in a loop or more than once is not reported.
program could_be_associate
implicit none
real, target :: a(4) = [1.0, 2.0, 3.0, 4.0]
real, pointer :: head(:)
real, pointer :: item
integer :: i
head => a(1:2)                           ! expect: could-be-associate
head = 10*head
print *, a
do i = 1, size(a)
   item => a(i)
   item = item + 1
end do
print *, a
end program could_be_associate
