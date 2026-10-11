program intent_spellings
implicit none
integer :: compact, spaced, continued, values(3)
logical :: compact_changed, spaced_changed, continued_changed
compact = 3
spaced = 3
continued = 3
values = [1, 2, 3]
call bump_compact(compact, compact_changed)
call bump_spaced(spaced, spaced_changed)
call bump_continued(continued, continued_changed)
call bump_array(values)
print *, compact, spaced, continued
print *, values
print *, compact_changed, spaced_changed, continued_changed
contains
pure subroutine bump_compact(compact, changed)
integer, intent(inout) :: compact
logical, intent(out) :: changed
compact = compact + 2
changed = compact > 3
end subroutine bump_compact
pure subroutine bump_spaced(spaced, changed)
integer, InTeNt(In OuT) :: spaced
logical, intent(out) :: changed
spaced = spaced + 2
changed = spaced > 3
end subroutine bump_spaced
pure subroutine bump_continued(continued, changed)
integer, intent(in & ! Continue the attribute.
! Intervening comment.
 & out) :: continued
logical, intent(out) :: changed
continued = continued + 2
changed = continued > 3
end subroutine bump_continued
pure subroutine bump_array(values)
integer, intent(in out) :: values(:)
values = values + 2
end subroutine bump_array
end program intent_spellings
