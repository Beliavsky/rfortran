program internal_integer_read
implicit none
character(len=80) :: text
integer :: n, ios
integer(kind=1) :: small
integer(kind=2) :: short
integer(kind=8) :: wide
text='42'
n=91
read(text,*,iostat=ios) n
if (ios > 0) ios=1
print *, ios
if (ios == 0) print *, n
text='  -123, ignored'
n=91
read(text,*,iostat=ios) n
if (ios > 0) ios=1
print *, ios
if (ios == 0) print *, n
text='+7 / ignored'
n=91
read(text,*,iostat=ios) n
if (ios > 0) ios=1
print *, ios
if (ios == 0) print *, n
text=''
n=91
read(text,*,iostat=ios) n
if (ios > 0) ios=1
print *, ios
if (ios == 0) print *, n
text='   '
n=91
read(text,*,iostat=ios) n
if (ios > 0) ios=1
print *, ios
if (ios == 0) print *, n
text=','
n=91
read(text,*,iostat=ios) n
if (ios > 0) ios=1
print *, ios
if (ios == 0) print *, n
text='/'
n=91
read(text,*,iostat=ios) n
if (ios > 0) ios=1
print *, ios
if (ios == 0) print *, n
text='2*19'
n=91
read(text,*,iostat=ios) n
if (ios > 0) ios=1
print *, ios
if (ios == 0) print *, n
text='2*'
n=91
read(text,*,iostat=ios) n
if (ios > 0) ios=1
print *, ios
if (ios == 0) print *, n
text='2* 999'
n=91
read(text,*,iostat=ios) n
if (ios > 0) ios=1
print *, ios
if (ios == 0) print *, n
text='0*3'
n=91
read(text,*,iostat=ios) n
if (ios > 0) ios=1
print *, ios
if (ios == 0) print *, n
text='-2*3'
n=91
read(text,*,iostat=ios) n
if (ios > 0) ios=1
print *, ios
if (ios == 0) print *, n
text='+2*3'
n=91
read(text,*,iostat=ios) n
if (ios > 0) ios=1
print *, ios
if (ios == 0) print *, n
text='2**3'
n=91
read(text,*,iostat=ios) n
if (ios > 0) ios=1
print *, ios
if (ios == 0) print *, n
text='1.0'
n=91
read(text,*,iostat=ios) n
if (ios > 0) ios=1
print *, ios
if (ios == 0) print *, n
text='2x'
n=91
read(text,*,iostat=ios) n
if (ios > 0) ios=1
print *, ios
if (ios == 0) print *, n
text='+'
n=91
read(text,*,iostat=ios) n
if (ios > 0) ios=1
print *, ios
if (ios == 0) print *, n
text='3e2'
n=91
read(text,*,iostat=ios) n
if (ios > 0) ios=1
print *, ios
if (ios == 0) print *, n
text='2147483647'
n=91
read(text,*,iostat=ios) n
if (ios > 0) ios=1
print *, ios
if (ios == 0) print *, n
text='-2147483648'
n=91
read(text,*,iostat=ios) n
if (ios > 0) ios=1
print *, ios
if (ios == 0) print *, n
text='2147483648'
n=91
read(text,*,iostat=ios) n
if (ios > 0) ios=1
print *, ios
if (ios == 0) print *, n
text='-2147483649'
n=91
read(text,*,iostat=ios) n
if (ios > 0) ios=1
print *, ios
if (ios == 0) print *, n
text='9223372036854775808'
n=91
read(text,*,iostat=ios) n
if (ios > 0) ios=1
print *, ios
if (ios == 0) print *, n
text='9999999999999999999999999999999999999999'
n=91
read(text,*,iostat=ios) n
if (ios > 0) ios=1
print *, ios
if (ios == 0) print *, n
text='3/garbage'
n=91
read(text,*,iostat=ios) n
if (ios > 0) ios=1
print *, ios
if (ios == 0) print *, n
text='8!comment'
n=91
read(text,*,iostat=ios) n
if (ios > 0) ios=1
print *, ios
if (ios == 0) print *, n
text='2*+7'
n=91
read(text,*,iostat=ios) n
if (ios > 0) ios=1
print *, ios
if (ios == 0) print *, n
text='2*-8'
n=91
read(text,*,iostat=ios) n
if (ios > 0) ios=1
print *, ios
if (ios == 0) print *, n
text='127'
read(text,*,iostat=ios) small
if (ios > 0) ios=1
print *, ios
if (ios == 0) print *, small
text='-128'
read(text,*,iostat=ios) small
if (ios > 0) ios=1
print *, ios
if (ios == 0) print *, small
text='128'
read(text,*,iostat=ios) small
if (ios > 0) ios=1
print *, ios
if (ios == 0) print *, small
text='-129'
read(text,*,iostat=ios) small
if (ios > 0) ios=1
print *, ios
if (ios == 0) print *, small
text='1000*7'
read(text,*,iostat=ios) small
if (ios > 0) ios=1
print *, ios
if (ios == 0) print *, small
text='32767'
read(text,*,iostat=ios) short
if (ios > 0) ios=1
print *, ios
if (ios == 0) print *, short
text='-32768'
read(text,*,iostat=ios) short
if (ios > 0) ios=1
print *, ios
if (ios == 0) print *, short
text='32768'
read(text,*,iostat=ios) short
if (ios > 0) ios=1
print *, ios
if (ios == 0) print *, short
text='-32769'
read(text,*,iostat=ios) short
if (ios > 0) ios=1
print *, ios
if (ios == 0) print *, short
text='9223372036854775807'
read(text,*,iostat=ios) wide
if (ios > 0) ios=1
print *, ios
if (ios == 0) print *, wide
text='-9223372036854775808'
read(text,*,iostat=ios) wide
if (ios > 0) ios=1
print *, ios
if (ios == 0) print *, wide
text='9223372036854775808'
read(text,*,iostat=ios) wide
if (ios > 0) ios=1
print *, ios
if (ios == 0) print *, wide
text='-9223372036854775809'
read(text,*,iostat=ios) wide
if (ios > 0) ios=1
print *, ios
if (ios == 0) print *, wide
text='2*-9223372036854775808'
read(text,*,iostat=ios) wide
if (ios > 0) ios=1
print *, ios
if (ios == 0) print *, wide
text='314'
read(text,*) n
print *, n
print *, parse_local()
contains
pure integer function parse_local() result(value)
character(len=3) :: local_text
local_text='567'
read(local_text,*) value
end function parse_local
end program
