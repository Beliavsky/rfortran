program test_list_directed_output
    implicit none
    integer :: a(2)
    real :: r(2)
    character(len=1) :: s
    a(1) = 7
    a(2) = 8
    r(1) = 1.5
    r(2) = 2.5
    s = "B"
    print *, "n, n^2 =", 5, 25
    print *, 1, 2, 3
    print *, "A", s, "C"
    print *, "left ", " right", 4
    print *, "", "", 9, "", "Z"
    print *, "array:", a, 9
    print *, "real:", r, 3.5
    print *, "logical:", .true., .false.
end program test_list_directed_output
