program test_character_active_extent
implicit none
character(4)::words(4)
character(4),allocatable::keys(:)
integer::groups(4)
words=['red ','blue','red ','none']
call make_groups(words,keys,groups)
print *,groups
contains
subroutine make_groups(words,keys,groups)
character(*),intent(in)::words(:)
character(*),allocatable,intent(out)::keys(:)
integer,intent(out)::groups(:)
integer::n
n=2
allocate(keys(n))
keys(1)='red'
keys(2)='blue'
groups=positions(words,keys)
end subroutine make_groups
pure function positions(words,keys) result(groups)
character(*),intent(in)::words(:),keys(:)
integer::groups(size(words)),i
forall(i=1:size(words)) groups(i)=position(words(i),keys)
end function positions
pure function position(word,keys) result(found)
character(*),intent(in)::word,keys(:)
integer::found,j
found=0
do j=1,size(keys)
   if(keys(j)==word) then
      found=j
      return
   end if
end do
end function position
end program test_character_active_extent
