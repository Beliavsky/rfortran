program stepped
implicit none
integer,parameter :: stride=2
integer :: i,j,n,total,values(4)
n=5
total=0
do i=1,n,stride
 n=0
 if (i==3) cycle
 total=total+i
end do
write(*,'(2I5)') total,i
total=0
do i=6,1,-stride
 do j=1,3,2
  if(j==1) cycle
  total=total+i+j
 end do
 if(i==4) cycle
 total=total+1
end do
write(*,'(3I5)') total,i,j
do i=1,6,-1
 total=999
end do
write(*,'(2I5)') total,i
do i=6,1,2
 total=999
end do
write(*,'(2I5)') total,i
do i=10,1,-3
 if(i==7) exit
end do
write(*,'(I5)') i
! End expression sees the old value of the control variable.
i=5
total=0
do i=1,i,2
 total=total+i
end do
write(*,'(2I5)') total,i
write(*,'(I5)') reverse_sum(5)
values = [1,2,-1,-2]
call last_positive(values,n)
write(*,'(I5)') n
! CYCLE in SELECT CASE and BLOCK still advances the surrounding loop.
total=0
do i=5,1,-2
 select case(i)
 case(3)
  block
   cycle
  end block
 case default
  total=total+i
 end select
end do
write(*,'(2I5)') total,i
! Every CASE terminates its branch; an EXIT keeps the current index.
do i=3,1,-1
 select case(i)
 case(3)
  cycle
 case default
  exit
 end select
end do
write(*,'(I5)') i
! No matching CASE and no DEFAULT continues after SELECT.
total=0
do i=1,3,1
 select case(i)
 case(2)
  total=total+10
 end select
 total=total+i
end do
write(*,'(2I5)') total,i
contains
pure subroutine last_positive(values,last)
integer,intent(in)::values(:)
integer,intent(out)::last
integer::k
last=0
do k=size(values),1,-1
 if(values(k)>0) then
  last=k
  exit
 end if
end do
end subroutine last_positive
pure integer function reverse_sum(n) result(s)
integer,intent(in)::n
integer::k
s=0
do k=n,1,-2
 s=s+k
end do
end function reverse_sum
end program stepped
