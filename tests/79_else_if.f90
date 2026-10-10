program elseif_chains
implicit none
integer :: i, value, elseif
elseif=7
print '(i0)', elseif
do i=1,4
 if (i == 1) then
  value=10
 else if (i == 2) then
  value=20
 ElSeIf (i == 3) then
  value=30
 else
  value=40
 end if
 print '(i0)', value
end do
value=99
if (.false.) then
 value=1
else & ! a continued branch
! comment between continuation lines
 & if (.false.) then
 value=2
end if
print '(i0)', value
if (.false.) then
 value=0
else
 if (.true.) then
  value=50
 else if (.false.) then
  value=0
 end if
end if
print '(i0)', value
if (.false.) then; value=0; else; if (.true.) then; value=60; end if; end if
print '(i0)', value
end program elseif_chains
