program test_tokenize
implicit none
character(:),allocatable :: tokens(:),separators(:)
integer,allocatable :: first(:),last(:)
character(*),parameter :: text="parsley,sage,rosemary,&thyme"
integer :: i
call tokenize(text,",&",tokens,separators)
print *,"number of tokens:",size(tokens),"element length:",len(tokens)
do i=1,size(tokens)
    print "('[',a,']')",tokens(i)
end do
print "('separators: ',*(a))",separators
call tokenize(string=text,set=",&",first=first,last=last)
print *,"first:",first
print *,"last:",last
end program
