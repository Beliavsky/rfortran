program integer_huge
implicit none
integer,parameter :: limit=huge(0)
integer(kind=1) :: i1
integer(kind=2) :: i2
integer :: i4
integer(kind=8) :: i8
integer :: a(3),empty(0)
i1=1
i2=2
i4=4
i8=8
a=0
print *,limit,huge(-123),-huge(0)-1
print *,huge(i1),huge(i2),huge(i4),huge(i8)
print *,kind(i1),kind(i2),kind(i4),kind(i8)
print *,kind(huge(i1)),kind(huge(i2)),kind(huge(i4)),kind(huge(i8))
print *,huge(a),huge(1+2),i1,i2,i4,i8,sum(a)
print *,huge(empty),sum(empty)
end program integer_huge
