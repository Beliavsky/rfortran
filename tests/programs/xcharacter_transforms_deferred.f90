implicit none
integer::n
character(:),allocatable::a(:),b(:)
n=3
allocate(character(len=n)::a(3))
a=['one','two','six']
b=cshift(a,1)
print *,len(b),size(b)
print '(3(a,a,a))','[',b(1),']','[',b(2),']','[',b(3),']'
a=eoshift(a,-1)
print *,len(a),size(a)
print '(3(a,a,a))','[',a(1),']','[',a(2),']','[',a(3),']'
deallocate(a)
n=2
allocate(character(len=n)::a(2))
a=['ab','cd']
b=cshift(array=a,shift=-1)
print *,len(b),size(b)
print '(2(a,a,a))','[',b(1),']','[',b(2),']'
end
