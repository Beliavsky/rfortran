program output_implied_do
implicit none
integer :: i, j, n, step, a(4)
character(len=5) :: word
n=4
step=-2
a=[10,20,30,40]
word='hi'
write(*,*) 'values:', (a(i),i=1,n), 'end', i
print *, (i, a(i)+1, i=4,1,step), i
write(*,*) 'nested:', ((i*10+j,j=1,2),i=1,2), i, j
write(*,*) 'empty:', (i,i=3,1), 'done', i
write(*,*) 'reverse empty:', (i,i=1,3,-1), 'done', i
write(*,*) 'chars:', (trim(word)//' ',i=1,3), 'done'
word=' '
write(*,*) 'blank:', (trim(word)//'.',i=1,2)
write(*,*) (i,i=2,1)
write(*,*) 'array:', a, (i,i=1,2), a(4:2:-2)
i=2
write(*,*) (i,i=i,n), i
end program output_implied_do
