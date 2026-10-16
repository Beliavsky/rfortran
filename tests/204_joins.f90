program test_joins
implicit none
character(4)::words(4),empty(0)
integer::n
words(1)='a'
words(2)='bb'
n=2
if(len(join(words(:n),':'))/=4)stop 1
if(join(words(:n),':')/='a:bb')stop 2
words(1)='abc'
words(2)='d'
if(len(join(words(:n),':'))/=5)stop 3
if(join(words(:n),':')/='abc:d')stop 4
if(len(join(empty,':'))/=0)stop 5
n=0
if(len(join(words(:n),':'))/=0)stop 6
print *,'joins tests passed'
contains
function join(words,sep) result(str)
character(*),intent(in)::words(:),sep
character((size(words)-1)*len(sep)+sum(len_trim(words)))::str
integer::i,nw
nw=size(words)
str=''
if(nw<1)return
write(str,'(10000(a))')trim(words(1)),(sep//trim(words(i)),i=2,nw)
end function
end program
