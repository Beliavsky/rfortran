program main
    implicit none
    integer :: i
    block
        do i = 1, 10
            print *, "Inside block: ", i
        end do
    end block
    print *, "Exited block"
    print *, "i: ", i
end program main
