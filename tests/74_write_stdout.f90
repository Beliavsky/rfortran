program write_stdout
implicit none
integer, parameter :: stdout=6
integer :: write, a(4), matrix(2,2), next
character(len=3) :: label
write=7
a=[1,2,3,4]
matrix(:,1)=[11,12]
matrix(:,2)=[21,22]
label='abc'
write(*,*) 'value',write
write(6,*) a
write(stdout,*) a(4:1:-2),99
write(*,*) matrix(:,2)
write(*,*) .true.,.false.,1.5
write(*,'(a,i3,f6.2,l2)') label,7,1.5,.true.
write(*,*) ! A blank record must not consume the following assignment.
next=42
write(*,*) next
if (next==42) write(*,*) 'conditional'
if (next==0) write(*,*)
write(*,*); next=43
write(*,*) next
WrItE(*,*) &
 & 'continued',next
write(6,'("done")')
stop
write(*,*) 'must not execute'
end program write_stdout
