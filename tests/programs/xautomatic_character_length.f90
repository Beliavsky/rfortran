program main
implicit none
integer :: n
read *, n
call work(n)
n=2
call work(n)
n=0
call work(n)
n=-3
call work(n)
contains
subroutine work(n)
integer,intent(inout) :: n
character(len=n) :: text
print *, len(text)
n=1
text='abcdef'
print '(a)', '['//text//']'
text='Z'
print '(a)', '['//text//']'
end subroutine
end program
