program entity_character_lengths
implicit none
integer,parameter::n=3
character(2)::a*3,b,c*(n+1)
character,parameter::tag*3='abcde',padded*5='xy',inferred*(*)='four'
character(len=len(a))::same
character(len=max(1,len(a)+2))::longer
character::empty*(0)
a='hello'
b='world'
c='q'
same=a
longer=a
empty='ignored'
print *,len(a),len(b),len(c),len(same),len(longer),len(empty)
print *, '['//a//']['//b//']['//c//']'
print *, '['//same//']['//longer//']['//empty//']'
print *, '['//tag//']['//padded//']['//inferred//']'
print *,trim(tag),trim(padded),len_trim(padded)
end program entity_character_lengths
