! Repro for xacv.f90:25771, calling read_words_line defined at line 4935.
! Self-contained: a scratch file supplies "3 alpha beta gamma".
! Expected output, one item per line: 3, alpha, beta, gamma.
! Former rfort error: ALLOCATABLE output specialization requires a bounded rank-one array
! input or fixed local array.
module read_words_allocatable_mod
implicit none
contains
subroutine read_words_line(iu,words,nread)
integer,intent(in)::iu
character(len=*),allocatable,intent(out)::words(:)
integer,intent(out),optional::nread
character(len=100)::text
integer::nw
read(iu,'(a)') text
read(text,*) nw
nw=max(nw,0)
allocate(words(nw))
if(nw>0) read(text,*) nw,words
if(present(nread)) nread=size(words)
end subroutine
end module

program test_read_words_allocatable
use read_words_allocatable_mod
implicit none
character(len=20),allocatable::words(:)
integer::i
open(10,status='scratch',action='readwrite')
write(10,'(a)') '3 alpha beta gamma'
rewind(10)
call read_words_line(10,words)
close(10)
print *,size(words)
do i=1,size(words)
   print '(a)',trim(words(i))
end do
end program
