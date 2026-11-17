! Warning:    enum-exhaustive
! Enabled by: -Wall
! gfortran: no (Fortran 2023 enumeration types are not supported yet)
! A SELECT CASE on an enumeration type, without CASE DEFAULT, that misses
! some of the type's values. Adding a value to the type then silently skips
! the construct; the warning names the missing values.
program enum_exhaustive
implicit none
enumeration type :: colour
   enumerator :: red, green, blue
end enumeration type
type(colour) :: c
c = green
select case (c)       ! expect: enum-exhaustive
case (red)
   print *, "red"
case (green)
   print *, "green"
end select
select case (c)
case (red)
   print *, "red"
case (green, blue)
   print *, "green or blue"
end select
end program enum_exhaustive
