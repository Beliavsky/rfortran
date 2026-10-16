! echo: "NaN"
! echo: "Infinity"
! echo: "-Infinity"
! echo: "-0.0"
program test_isnan
implicit none
integer,parameter::dp=selected_real_kind(15)
real(dp)::x(4),nan,pos_inf,neg_inf,zero
real::r
logical::mask(4)
read(*,*) nan
read(*,*) pos_inf
read(*,*) neg_inf
read(*,*) zero
x=[nan,pos_inf,neg_inf,zero]
mask=isnan(x=x)
if (any(mask.neqv.[.true.,.false.,.false.,.false.])) error stop 'array isnan'
if (count(isnan(x))/=1) error stop 'count isnan'
r=real(x(1))
if (.not.isnan(r)) error stop 'real isnan'
if (.not.check(x(1))) error stop 'pure isnan'
if (isnan(0.0_dp)) error stop 'finite isnan'
print *, 'isnan tests passed'
contains
pure logical function check(value)
real(dp),intent(in)::value
check=isnan(value)
end function check
end program test_isnan
