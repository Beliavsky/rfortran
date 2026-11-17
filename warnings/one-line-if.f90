! Warning:    one-line-if
! Enabled by: -Wone-line-if only
! A one-line IF statement, for projects that always write the IF block
! form, which takes further statements without restructuring. This is the
! reverse of -Wcould-be-one-line-if, so -Weverything leaves it out.
program one_line_if
implicit none
real :: x
x = 4.0
if (x > 1) x = x - 1         ! expect: one-line-if
if (x > 0) then
   x = sqrt(x)
end if
print *, x
end program one_line_if
