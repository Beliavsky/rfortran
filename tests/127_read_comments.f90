module m
implicit none
contains
subroutine read_comments(iu,marker,echo,comments)
! read comments from a file, treating lines beginning with marker and followed by a space or newline to be comments
integer          , intent(in) :: iu
character (len=*), intent(in) :: marker
character (len=*), intent(out), allocatable, optional :: comments(:)
logical, intent(in), optional  :: echo
integer, parameter             :: max_comments = 4
character (len=32)            :: lines(max_comments),word,file_name
integer                        :: i,ierr,nread
logical                        :: echo_
echo_ = .false.
if (present(echo)) echo_ = echo
nread = max_comments
inquire (unit=iu,name=file_name)
if (echo_) write (*,*) "comments from file " // trim(file_name)
do i=1,max_comments
   read (iu,"(a)",iostat=ierr) lines(i)
   if (ierr /= 0) then
      nread = i-1
      exit
   end if
   read (lines(i),*) word
   if (word /= marker) then
      backspace (iu)
      nread = i-1
      exit
   end if
   if (echo_) write (*,*) trim(lines(i))
end do
if (present(comments)) then
   allocate (comments(nread))
   comments = lines(1:nread)
end if
if (echo_) write (*,*)
end subroutine read_comments
end module m

program test_read_comments
use m, only: read_comments
implicit none
integer :: iu, i
character(len=32), allocatable :: comments(:)
character(len=32) :: line

! Use a scratch file so the example needs no input file and leaves no file behind.
open(newunit=iu, status="scratch", action="readwrite", form="formatted")
write(iu,"(a)") "! first comment", "!", "!not_a_comment", "42"
rewind(iu)

call read_comments(iu, "!", comments=comments)
if (size(comments) /= 2) error stop "expected two comments"
if (trim(comments(1)) /= "! first comment") error stop "wrong first comment"
if (trim(comments(2)) /= "!") error stop "marker-only comment not recognized"
print *, "comments read:", size(comments)
do i=1,size(comments)
   print *, trim(comments(i))
end do
read(iu,"(a)") line
if (trim(line) /= "!not_a_comment") then
   print "('line: ',a)", "'" // trim(line) // "'"
   error stop "first non-comment was consumed"
end if
print *, "first non-comment: ", trim(line)

! Both OPTIONAL arguments may be omitted.
rewind(iu)
call read_comments(iu, "!")
read(iu,"(a)") line
if (trim(line) /= "!not_a_comment") error stop "optional-argument test failed"

! No leading comments: return an allocated empty array and preserve the data.
call read_comments(iu, "!", echo=.false., comments=comments)
if (.not. allocated(comments)) error stop "empty comments array not allocated"
if (size(comments) /= 0) error stop "expected no comments before data"
read(iu,"(a)") line
if (trim(line) /= "42") error stop "data record was consumed"

close(iu)
print *, "read_comments tests passed"
end program test_read_comments
