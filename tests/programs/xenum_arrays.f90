program enum_arrays
implicit none
enum, bind(c) :: season
   enumerator :: spring=5, summer=7, autumn, winter
end enum
type calendar
   type(season) :: current=summer
   type(season) :: following(2)
end type
type(season) :: seasons(3)
type(calendar) :: plan

seasons=[season :: spring,summer,autumn]
print *,int(seasons)
seasons=[spring,season(6),season(8)]
print *,int(seasons)
plan=calendar(following=[season :: spring,season(6)],current=ior(spring,2))
print *,int(plan%current),int(plan%following)
end program enum_arrays
