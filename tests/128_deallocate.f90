program deallocation
implicit none
integer,allocatable::a(:),empty(:)
character(len=:),allocatable::s
integer::i
print *,allocated(a)
allocate(a(2),empty(0))
a=[1,2]
s='hello'
print *,size(a),sum(a),allocated(empty)
deallocate(a,s,empty)
print *,allocated(a),allocated(s),allocated(empty)
allocate(a(3))
a=4
call reset(a)
print *,a,allocated(a)
deallocate(a)
do i=1,2
  allocate(a(0))
  print *,allocated(a),size(a)
  if (allocated(a)) deallocate(a)
end do
s='new'
print *,s,len(s)
deallocate(s)
print *,allocated(a),allocated(s)
contains
pure subroutine reset(a)
integer,allocatable,intent(inout)::a(:)
deallocate(a)
allocate(a(2))
a=9
end subroutine reset
end program deallocation
