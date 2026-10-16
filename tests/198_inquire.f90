program test_inquire
implicit none
integer::u,n,ios
logical::opened
inquire(unit=20,opened=opened,number=n,iostat=ios)
if (opened .or. n/=-1 .or. ios/=0) error stop 'unopened unit'
open(20,status='scratch')
u=20
inquire(iostat=ios,number=n,opened=opened,unit=u)
if (.not. opened .or. n/=20 .or. ios/=0) error stop 'connected unit'
close(20)
inquire(20,opened=opened)
if (opened) error stop 'closed unit'
open(newunit=u,status='scratch')
inquire(unit=u,number=n,opened=opened,iostat=ios)
if (.not. opened .or. n/=u .or. ios/=0) error stop 'newunit'
close(u)
inquire(unit=u,number=n,opened=opened)
if (opened .or. n/=-1) error stop 'closed newunit'
inquire(unit=5,opened=opened,number=n,iostat=ios)
if (.not. opened .or. n/=5 .or. ios/=0) error stop 'stdin'
inquire(unit=6,opened=opened)
if (.not. opened) error stop 'stdout'
inquire(unit=2048,opened=opened,number=n,iostat=ios)
if (opened .or. n/=-1 .or. ios/=0) error stop 'unopened large unit'
u=-1
inquire(unit=u,iostat=ios)
if (ios==0) error stop 'invalid unit status'
print *, 'inquire tests passed'
end program test_inquire
