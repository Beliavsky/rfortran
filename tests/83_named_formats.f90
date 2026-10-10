module output_formats
implicit none
character(len=*), parameter :: integer_format = "('value=',i3)"
end module output_formats
program named_formats
use output_formats, only: integer_format
implicit none
character(len=30), parameter :: padded = "('real=',f6.2)"
character(len=*), parameter :: empty_items = "('done')"
write (*,integer_format) 42
print integer_format, -7
write (6,padded) 1.25
write (*,'(' // 'i3' // ')') 9
print '(' // 'a' // ')', 'hello'
write (*,empty_items)
write (*,*) 123
end program named_formats
