module first_character_mod
implicit none
character(9)::vec
contains
pure function first(vec) result(text)
character(*),intent(in)::vec(:)
character(len=len(vec))::text
if(size(vec)>0)then
   text=vec(1)
else
   text='?'
end if
end function
end module
program test_first_character
use first_character_mod,only:first
implicit none
character(3) :: words(2)
words=['cat','dog']
print *, '['//first(words)//']'
print *, '['//first([character(3)::])//']'
end program
