module runtime_where_fill_mod
use, intrinsic :: ieee_arithmetic, only: ieee_value,ieee_positive_inf,ieee_quiet_nan,ieee_is_nan
implicit none
type::table
double precision,allocatable::values(:,:)
contains
procedure::fillna
end type
contains
subroutine fillna(self,value)
class(table),intent(inout)::self
double precision,intent(in)::value
where(ieee_is_nan(self%values)) self%values=value
end subroutine
end module

program test_runtime_where_fill
use runtime_where_fill_mod
implicit none
type(table)::a,original
double precision::inf,nan
inf=ieee_value(0d0,ieee_positive_inf)
nan=ieee_value(0d0,ieee_quiet_nan)
allocate(a%values(2,3))
a%values=1d0
a%values(2,3)=inf
a%values(1,1)=nan
original=a
call a%fillna(-9d0)
print *,a%values(2,3)==inf, a%values(1,1)==-9d0
print *,a%values(2,1)==1d0, a%values(1,2)==1d0
print *,original%values(2,3)==inf
end
