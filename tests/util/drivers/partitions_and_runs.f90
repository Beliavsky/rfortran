! Differential driver: compare unchanged util.f90 procedures with gfortran.
program test_partitions_and_runs
use util_mod, only:partition,max_consecutive
implicit none
integer :: n, npart
print *,partition(10,1),partition(10,3),partition(3,5),size(partition(10,0))
n=10
do npart=0,4
   print *,partition(n,npart)
end do
print *,max_consecutive([1,1,0,0,0,1,1,1,0]>0)
print *,max_consecutive([1,1,1]>0),max_consecutive([0,0,0]>0)
print *,max_consecutive([logical::])
end program test_partitions_and_runs
