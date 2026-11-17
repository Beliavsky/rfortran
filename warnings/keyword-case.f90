! Warning:    keyword-case
! Enabled by: -Wstyle
! Keywords written in more than one case in a file. Pick one case and use it
! throughout; the warning is given once per file, at the first keyword in
! the other case.
program keyword_case
implicit none
integer :: n
n = 2
PRINT *, n           ! expect: keyword-case
print *, n + 1
end program keyword_case
