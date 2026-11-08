! Run with --parallel --parallel-report and RFORT_NUM_THREADS=4.
implicit none
integer :: i, t, a(9001)
integer(kind=8) :: visited, kept
a = 0
t = 7
visited = 3
kept = 5
do concurrent(i=1:9001) local_init(t) reduce(+:visited,kept)
   visited = visited + 1
   if (mod(i,2)==0) then
      t = 99
      cycle
   end if
   a(i) = i + t
   kept = kept + 1
end do
if (visited/=9004.or.kept/=4506.or.t/=7) stop 1
do i=1,9001
   if (mod(i,2)==0) then
      if (a(i)/=0) stop 2
   else
      if (a(i)/=i+7) stop 3
   end if
end do
print *, "PASS"
end
