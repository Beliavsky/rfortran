! Warning:    logical-comparison
! Enabled by: -Wextra, -Wstyle
! A LOGICAL value compared with .true. or .false.. `flag .eqv. .true.` is
! just `flag`, and `flag .eqv. .false.` is `.not. flag`. (`flag == .true.`
! is not standard Fortran at all.)
program logical_comparison
implicit none
logical :: done
done = .false.
if (done .eqv. .false.) print *, "not done"      ! expect: logical-comparison
if (.not. done) print *, "not done"
end program logical_comparison
