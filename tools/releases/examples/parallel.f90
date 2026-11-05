implicit none
integer :: i,a(10000)
do concurrent(i=1:10000) shared(a) default(none)
   a(i)=i
end do
print *,sum(a)
end
