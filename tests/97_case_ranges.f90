program case_ranges
implicit none
integer::i,total
integer,parameter::lo=2,hi=4
integer(8)::wide
total=0
do i=-2,7
select case(i)
case(:-1)
total=total+10
case(0,lo:hi,6:7)
total=total+1
case default
total=total+100
end select
end do
print *,total
select case(4)
case(1,2:3,4:5)
print *,42
case default
print *,-1
end select
i=6
select case(i+1)
case(hi+2:)
print *,7
case default
print *,-1
end select
wide=2147483647
wide=wide+1
select case(wide)
case(:0)
print *,-1
case(2147483647:)
print *,wide
end select
do i=1,5
select case(i)
case(1:2)
cycle
case(4:)
exit
case default
print *,i
end select
end do
print *,i
end program case_ranges
