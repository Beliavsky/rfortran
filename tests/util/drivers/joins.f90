! Differential driver: compare unchanged util.f90 procedures with gfortran.
program test_joins
use util_mod, only:join,join_quote,join_csv,join_suffix,join_csv_suffix,concat
implicit none
character(4)::words(4),empty(0)
integer::n
print *,join(['a ','bb'],':')
print *,join_quote(['a ','bb'],':',"'")
print *,join_csv(['a ','bb'])
print *,join_suffix(['a ','bb'],'!')
print *,join_csv_suffix(['a ','bb'],'!')
print *,trim(concat(['a ','bb']))
print *,trim(concat(['a ','bb'],sep=':',cquote="'"))
words(1)='a'
words(2)='bb'
n=2
print *,len(join(words(:n),':')),'['//join(words(:n),':')//']'
print *,join_quote(words(:n),':',"'")
print *,join_csv(words(:n))
print *,join_suffix(words(:n),'!')
print *,join_csv_suffix(words(:n),'!')
words(1)='abc'
words(2)='d'
print *,len(join(words(:n),':')),'['//join(words(:n),':')//']'
print *,len(join(empty,':')),'['//join(empty,':')//']'
n=0
print *,len(join(words(:n),':')),'['//join(words(:n),':')//']'
end program test_joins
