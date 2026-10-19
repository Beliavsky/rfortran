program test_split
implicit none
character(*),parameter :: text="one,two,,three"
integer :: pos,first
pos=0
do
    first=pos+1
    call split(text,",",pos)
    print "('[',a,']')",text(first:pos-1)
    if(pos>len(text)) exit
end do
end program
