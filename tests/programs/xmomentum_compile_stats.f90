! Reduced compile-time probe from the momentum main; requires statistics_mod.
program xmomentum_compile_stats
use kind_mod, only: dp
use statistics_mod, only: print_stats, iimean
implicit none
real(dp) :: values(10,2)
character(5) :: labels(2)
values = 1.0_dp
labels = [character(5) :: "first", "last"]
call print_stats([iimean], values, labels)
end program
