program runtime_character_storage
implicit none
character(:), allocatable :: text
integer :: n
n = 6000
text = repeat('ab', n)
print *, len(text), text(1:2), text(len(text)-1:)
text = text // ' end'
print *, len(text), text(len(text)-3:)
text = text(1:6)
print *, len(text), text
text = ''
print *, allocated(text), len(text)
deallocate(text)
print *, allocated(text)
end program runtime_character_storage
