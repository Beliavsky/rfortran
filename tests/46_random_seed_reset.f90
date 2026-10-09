implicit none
double precision :: x,y
call random_number(x)
call random_seed()
call random_number(y)
print *, x == y
call random_number(y)
print *, x /= y
end
