program concurrent_locality
implicit none
integer :: i, scratch, offset, values(4)
i=99
scratch=-1
offset=10
do concurrent(i=1:4) local(scratch) local_init(offset) shared(values) default(none)
   scratch=i*i
   offset=offset+i
   values(i)=scratch+offset
end do
print *,values
print *,i,scratch,offset
end program
