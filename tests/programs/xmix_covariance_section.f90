implicit none
integer, parameter :: dp=kind(1d0)
type :: parameters
real(dp), allocatable :: cov(:,:,:)
end type
type(parameters) :: p
allocate(p%cov(2,2,2))
p%cov(:,:,1)=reshape([0.64_dp,0.3_dp,0.3_dp,1.0_dp],[2,2])
print *, p%cov(:,:,1)
end
