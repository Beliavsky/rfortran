program select_case_list
implicit none
integer, parameter :: euler=1, heun=2
integer :: method

do method=0,5
   select case(method)
   case(euler,heun)
      print *,method,10
   case(3:4)
      print *,method,20
   case default
      print *,method,99
   end select
end do
end program
