module character_values
implicit none
private
public :: prefix, padded, cut, nl, show
character(len=*), parameter :: prefix="util::", nl=achar(5+5)
character(len=5), parameter :: padded='xy', cut='abcdef'
contains
subroutine show()
character(len=*), parameter :: child=prefix//'child!'
print *, child
end subroutine show
end module character_values
program main
use character_values, only: prefix, padded, cut, nl, show
implicit none
character(*), parameter :: message=prefix//'ready', mark=char(65)
character, parameter :: single='xyz'
character(len=0), parameter :: empty='ignored'
print *, '['//message//']'
print *, '['//padded//']'
print *, '['//cut//']'
print *, mark//single//empty
print *, 'first'//nl//'second'
call show()
end program main
