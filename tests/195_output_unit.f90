program test_output_unit
implicit none
real::a(2,2)
a=reshape([1.0,2.0,3.0,4.0],[2,2])
call show(a,6)
contains
subroutine show(x,unit)
real,intent(in)::x(:,:)
integer,intent(in)::unit
character(20)::fmt
integer::i
fmt='(2f8.2)'
write(unit,*) 'matrix'
do i=1,size(x,1)
   write(unit,fmt) x(i,:)
end do
end subroutine show
end program test_output_unit
