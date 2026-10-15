! Differential driver: compare unchanged util.f90 procedures with gfortran.
program test_read_comments
use util_mod, only:read_comments
implicit none
character(1000),allocatable::comments(:)
integer::i,n
! The runner creates comments.txt separately for each compiler execution.
open(unit=20,file='comments.txt',status='old',action='read')
call read_comments(20,'#',echo=.false.,comments=comments)
read(20,*) n
close(20)
print *,size(comments),n
do i=1,size(comments)
print *,trim(comments(i))
end do
end program test_read_comments
