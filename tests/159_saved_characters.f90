call step()
call step()
contains
subroutine step()
character(4),save::s='a'
character(2)::t='long'
print *, '['//s//']','['//t//']',len(s),len_trim(s),index(s,'a')
s=trim(s)//'b'
t(2:2)='x'
end subroutine step
end
