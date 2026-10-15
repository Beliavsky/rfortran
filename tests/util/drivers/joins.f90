! Differential driver: compare unchanged util.f90 procedures with gfortran.
program test_joins
use util_mod, only:join,join_quote,join_csv,join_suffix,join_csv_suffix,concat
implicit none
print *,join(['a ','bb'],':')
print *,join_quote(['a ','bb'],':',"'")
print *,join_csv(['a ','bb'])
print *,join_suffix(['a ','bb'],'!')
print *,join_csv_suffix(['a ','bb'],'!')
print *,trim(concat(['a ','bb']))
print *,trim(concat(['a ','bb'],sep=':',cquote="'"))
end program test_joins
