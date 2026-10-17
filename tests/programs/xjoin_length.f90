program test_join_length
implicit none
character(4)::words(2)
character(30)::text
words=['a   ','bb  ']
text=join(words,':')
print *,len(join(words,':')),'['//trim(text)//']'
words=['abc ','d   ']
print *,len(join(words,':')),'['//join(words,':')//']'
contains
function join(words,sep) result(str)
character(*),intent(in)::words(:),sep
character((size(words)-1)*len(sep)+sum(len_trim(words)))::str
integer::i,nw
nw=size(words)
str=''
if(nw<1)return
write(str,'(10000(a))') trim(words(1)),(sep//trim(words(i)),i=2,nw)
end function
end program
