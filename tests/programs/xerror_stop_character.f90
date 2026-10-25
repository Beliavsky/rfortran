program main
implicit none
character(12), allocatable :: names(:)
integer :: k
names = ['first       ', 'missing     ']
k = 2
error stop "rename_cols: column not found: " // trim(names(k))
end program
