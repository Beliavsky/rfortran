program leair_test
use shape_mod
implicit none
type(shape)::sh
sh=constructor(5,.true.,100,200)
call sh%print()
print *,sh%isFilled()
end program
