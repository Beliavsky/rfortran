! Warning:    could-be-one-line-if
! Enabled by: -Weverything
! An IF block holding a single action statement, which a one-line IF writes
! more compactly, as `if (x > 0) x = sqrt(x)`, when it fits on the line.
! -Wone-line-if reports the reverse. A comment in the block, which the one
! line could not keep, suppresses the warning, so the marker is the comment
! line before it.
program could_be_one_line_if
implicit none
real :: x
x = 4.0
! expect-next: could-be-one-line-if
if (x > 0) then
   x = sqrt(x)
end if
if (x > 1) x = x - 1
print *, x
end program could_be_one_line_if
