! Reproduce CHARACTER array selection by an INTEGER vector subscript.
! util.f90 uses the same selection in select_vec_character:
!     call set_alloc((xx(iuse)),xx)
program xcharacter_vector_subscript
implicit none
character(len=4) :: words(3), selected(3)
integer :: iuse(3)

words = ["adj ", "noun", "verb"]
iuse = [3, 1, 3]
selected = words(iuse)
print '(3(a,1x))', selected
if (any(selected /= ["verb", "adj ", "verb"])) error stop 1
end program xcharacter_vector_subscript
