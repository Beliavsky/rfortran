! Warning:    bare-end
! Enabled by: -Wstyle
! END of a program, module, procedure or derived type without its name, as
! `end` or `end subroutine`. The name shows what is ending and lets the
! compiler check it.
program bare_end
implicit none
call hello()
call goodbye()
contains
subroutine hello()
print *, "hello"
end subroutine           ! expect: bare-end
subroutine goodbye()
print *, "goodbye"
end subroutine goodbye
end program bare_end
