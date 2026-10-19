program example
implicit none
enum, bind(c) :: season
   enumerator :: spring=5, summer=7, autumn, winter
end enum
type(season) :: current

print *, spring, summer, autumn, winter
current = autumn + 1
print *, int(current), current == winter
current = season(6)
select case (current)
case (spring)
   print *, 'spring'
case (summer:winter)
   print *, 'summer through winter'
case default
   print *, 'unnamed value', int(current)
end select
end program example
