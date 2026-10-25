program main
implicit none
type table
character(3),allocatable::columns(:)
end type
type(table)::t
character(5)::key
t%columns=['one','two','one']
key='one'
print *,findloc(t%columns,key,dim=1)
print *,findloc(array=t%columns,value=key,dim=1,back=.true.,kind=8)
print *,findloc(t%columns,'onex',dim=1)
print *,findloc(t%columns,'two',mask=[.true.,.false.,.true.])
end program
