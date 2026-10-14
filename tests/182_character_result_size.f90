! Reproduces the CHARACTER result-length blocker in util.f90:join_fixed.
program test_join_fixed
implicit none
print '(a)', '[' // join_fixed(['boy ', 'girl', 'a   ', 'tree'], 10) // ']'
contains
function join_fixed(words, nlen) result(text)
character(len=*), intent(in) :: words(:)
integer, intent(in) :: nlen
character(len=size(words)*nlen) :: text
integer :: i
character(len=100) :: fmt_t
write(fmt_t, "('(10000a',i0,')')") nlen
write(text, fmt_t) (trim(words(i)), i=1,size(words))
end function join_fixed
end program test_join_fixed
