character(9)::text
logical::back
integer::i
text='abc123abc'
do i=1,2
back=i==2
print *,scan(text,'abc',back),verify(text,'abc',back),index(text,'abc',back)
print *,index(text,'',back),scan(text,'',back),verify(text,'',back)
end do
print *,index('',''),index('','x'),scan('','x'),verify('','x')
print *,scan(text,' '),verify('   ',' ')
end
