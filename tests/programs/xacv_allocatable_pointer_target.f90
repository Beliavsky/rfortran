! Regression for ret_use => zret(i1:) in xacv.f90:25825.
! Expected output:
! 2
! 20 3
! 1 20 3
program test_allocatable_pointer_target
implicit none
double precision,allocatable,target::zret(:)
double precision,pointer::ret_use(:)
integer::n,i1
n=3
allocate(zret(n))
zret=[1d0,2d0,3d0]
i1=2
ret_use=>zret(i1:)
ret_use(1)=20d0
print *,size(ret_use)
print *,ret_use
print *,zret
nullify(ret_use)
deallocate(zret)
end program
