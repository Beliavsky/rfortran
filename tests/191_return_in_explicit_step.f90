! RETURN must leave an inlined explicit-step loop before its next increment.
program test_return_in_explicit_step
implicit none
print '(a)', '>' // trim(strip(',boy,,girl,,,  ',',')) // '<'
print '(a)', '>' // trim(strip(',,,',',')) // '<'
print *,partition(10,3)
contains
function strip(text,delimiter) result(clean)
character(*),intent(in)::text
character,intent(in)::delimiter
character(len(text))::clean
integer::i
clean=text
do i=len(text),1,-1
   if(clean(i:i)==' ') cycle
   if(clean(i:i)==delimiter)then
      clean(i:i)=' '
   else
      return
   end if
end do
end function
pure function partition(n,npart) result(positions)
integer,intent(in)::n,npart
integer::positions(npart),i
do i=1,npart
   positions(i)=nint(n*i/(npart+1.0))
end do
end function
end program
