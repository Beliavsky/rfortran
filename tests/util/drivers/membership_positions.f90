! Differential driver: compare unchanged util.f90 procedures with gfortran.
program test_membership_positions
use util_mod, only:pos_keep,intersection,num_missing,operator(.exclude.)
implicit none
print *,pos_keep(['a','c','d'],['a'])
print *,size(pos_keep(['a','c','d'],['d','a','c']))
print *,pos_keep(['a','c','d'],['e'])
print '(100(a,1x))',intersection(['a','c','d'],['b','c','a'])
print *,size(intersection(['a'],['b']))
print *,num_missing([10],[2,3,4,5,6]),num_missing([2,4,6],[2,3,4,5,6])
print '(100(a,1x))',['dog','cat','boy'] .exclude. ['cat']
print *,size(['dog','cat'] .exclude. ['dog','cat'])
end program test_membership_positions
