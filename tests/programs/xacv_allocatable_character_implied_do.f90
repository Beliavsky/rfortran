! Repro for the generated @alloc_block error at xacv.f90:4965.
! Expected output: alpha beta (on one line).
program test_allocatable_character_implied_do
implicit none
character(8), allocatable :: words(:)
call make_words(2, words)
deallocate(words)
contains
subroutine make_words(n, words)
integer, intent(in) :: n
character(*), allocatable, intent(out) :: words(:)
integer :: i, nread
if (n > 0) then
   allocate(words(n))
   words = 'alpha'
   if (n > 1) words(2) = 'beta'
else
   allocate(words(0))
end if
nread = size(words)
write(*,*) (trim(words(i)) // ' ', i=1,nread)
end subroutine make_words
end program test_allocatable_character_implied_do
