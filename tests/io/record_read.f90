program record_read
implicit none
character(len=8) :: text
character(len=10000) :: long_text
character(len=0) :: empty
character :: single
integer :: i, number
text = 'stale'
do i=1,3
  read (*,"(A)") text
  print '("[",a,"]")', text
end do
read (*,*) number
print '(i2)', number
read (5,"(a)") long_text
print '("[",a,"]")', long_text
read (*,"(a)") empty
print '("[",a,"]")', empty
read (*,"(a)") single
print '("[",a,"]")', single
end program
