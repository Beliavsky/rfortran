! Warning:    keyword-name
! Enabled by: -Wextra
! A variable, dummy argument or procedure named like a Fortran statement
! keyword or attribute, such as `block`, `exit`, `if` or `result`. Fortran
! has no reserved words, so this is legal, but it is harder to read.
program keyword_name
implicit none
integer :: block                     ! expect: keyword-name
integer :: block_size
block = 4
block_size = 8
print *, block, block_size
end program keyword_name
