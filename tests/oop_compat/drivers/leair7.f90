program leair_test
use shape_mod
implicit none
type(shape)::shp
type(rectangle)::rect
type(square)::sq
call shp%initialize(1,.true.,10,20)
call rect%initialize(2,.false.,100,200,11,22)
call sq%initialize(3,.false.,400,500)
print *,shp%color,shp%filled,shp%x,shp%y
print *,rect%color,rect%filled,rect%x,rect%y,rect%length,rect%width
print *,sq%color,sq%filled,sq%x,sq%y,sq%length,sq%width
end program
