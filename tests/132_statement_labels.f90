program main
implicit none
integer::i,total
10 total=0
do i=1,3
20 total=total+i
30 continue
end do
40 if (total==6) write(*,*) 'sum',total
block
50 print *,'block'
end block
60 if (.true.) continue
call hello()
contains
subroutine hello()
10 write(*,*) 'hello'
end subroutine hello
end program main
