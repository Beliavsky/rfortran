! Exercise the unchanged platform query, using the inherited environment.
program test_environment
use util_mod,only:unix
implicit none
print *,unix(),unix()
end program
