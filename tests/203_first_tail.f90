module first_tail_regression
implicit none
character(9) :: vec
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
pure function first_real(vec) result(y)
double precision,intent(in)::vec(:)
double precision::y
if(size(vec)>0)then
y=vec(1)
else
y=-999
end if
end function
end module
program test_first_tail
use first_tail_regression,only:first,first_real
implicit none
character(3)::words(2),empty_words(0)
double precision::empty_real(0),x
words=['cat','dog']
if(len(first(words))/=3)stop 1
if(first(words)/='cat')stop 2
if(first(empty_words)/='?  ')stop 3
if(len(first([character(3)::]))/=3)stop 4
if(first([character(3)::])/='?  ')stop 5
if(first_real(empty_real)/=-999d0)stop 6
x=3
if(x/2/=1.5d0)stop 7
print *,'first/tail tests passed'
end program
