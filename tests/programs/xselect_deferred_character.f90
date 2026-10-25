program select_deferred_character
implicit none
character(:), allocatable :: axis
axis = 'rows'
select case (axis)
case ('rows')
   print *, 'rows matched'
case ('cols', 'columns')
   error stop 'wrong case'
case default
   error stop 'deferred CHARACTER selector did not match'
end select
axis = 'columns'
select case (trim(axis))
case ('rows')
   error stop 'wrong case'
case ('cols', 'columns')
   print *, 'columns matched'
case default
   error stop 'CHARACTER expression selector did not match'
end select
end program
