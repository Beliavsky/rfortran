! Reduced from DataFrame xtable_stats: heap temporaries in an inlined function.
module correlation_example
implicit none
integer,parameter::dp=kind(1d0)
contains
pure function corr_mat(x) result(cor)
    ! return the correlation matrix of the columns of x(:,:)
    real(kind=dp), intent(in) :: x(:,:)
    real(kind=dp)             :: cor(size(x,2), size(x,2))
    real(kind=dp)             :: mean_vec(size(x,2)), std_vec(size(x,2))
    real(kind=dp)             :: centered_x(size(x,1), size(x,2))
    integer                   :: n, p

    n = size(x, 1)  ! Number of rows
    p = size(x, 2)  ! Number of columns

    ! Compute the mean of each column
    mean_vec = sum(x, dim=1) / n

    ! Center the matrix by subtracting the mean of each column
    centered_x = x - spread(mean_vec, dim=1, ncopies=n)

    ! Compute the standard deviation of each column
    std_vec = sqrt(sum(centered_x**2, dim=1) / (n - 1))

    cor = matmul(transpose(centered_x), centered_x) / (n - 1)
    cor = cor / spread(std_vec, dim=1, ncopies=p)
    cor = cor / spread(std_vec, dim=2, ncopies=p)
end function corr_mat
end module
program main
use correlation_example
implicit none
real(dp)::x(4,2)
x(:,1)=[1d0,2d0,3d0,4d0]
x(:,2)=2*x(:,1)
print *,corr_mat(x)
end
