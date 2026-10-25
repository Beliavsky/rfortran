program large_character_storage
implicit none
character(len=1200000) :: text
character(len=2) :: seed
read (*,"(a)") seed
text = repeat(seed,600000)
print *,len(text),text(1:2),text(len(text)-1:)
text = text(1:600000) // text(600001:)
print *,len_trim(adjustl(text)),len_trim(adjustr(text))
end program
