module colours
implicit none
enumeration type :: colour
   enumerator :: red, orange
   enumerator green
end enumeration type colour
contains
pure function advance(light) result(next_light)
type(colour),intent(in)::light
type(colour)::next_light
next_light=colour(mod(int(light),3)+1)
end function
end module

program main
use colours
implicit none
type(colour)::light=orange
integer::i
print *,int(red),int(light),int(huge(light))
print *,red<orange,green>=orange,light==orange
select case(light)
case(red)
   print *,"red"
case(orange:green)
   print *,"orange or green"
end select
light=advance(light)
print *,int(light)
do i=1,3
   light=colour(i)
   print *,int(light)
end do
end
