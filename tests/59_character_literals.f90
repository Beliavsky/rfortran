program character_literals
implicit none
integer :: i
i=1; i=i+1; print *, i
print '(a)', "\"
print '(a)', '\'
print '(a)', "C:\new\temp\"
print '(a)', "literal \n \t \r \\"
print '(a)', "He said ""hello""."
print '(a)', 'It''s Fortran.'
print '(a)', "'; ! & stay in string"
print '(a)', ''; print '(a)', 'done'
select case(i)
case(2); print *, "two"; i=i+1
case default; print *, "unexpected"
end select
print *, i
end program character_literals
