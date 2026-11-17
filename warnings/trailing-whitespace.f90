! Warning:    trailing-whitespace
! Enabled by: -Wstyle
! A free-form line ending in spaces or tabs, invisible in most editors and a
! source of spurious differences between versions. The warning is given
! once per file. The marked line ends in three spaces; keep them when
! editing this file.
program trailing_whitespace
implicit none
integer :: n
n = 2    ! expect: trailing-whitespace   
print *, n
end program trailing_whitespace
