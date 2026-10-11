program scalar_substrings
implicit none
character(8) :: s
character(3) :: t
s = 'abcdefgh'
s(2:8) = s(1:7)
print *, s == 'aabcdefg'
s = 'abcdefgh'
s(1:7) = s(2:8)
print *, s == 'bcdefghh'
s(:) = 'abcdefgh'
s(3:5) = 'X'
print *, '[',s,']',s=='abX  fgh'
s(3:5) = '12345'
print *, s=='ab123fgh'
s(2:1) = 'ignored'
print *, s=='ab123fgh',s(2:1)==''
t = s(3:5)
print *, '[',t,']','[',s(:2),']','[',s(6:),']'
s = 'abcdefgh'
s(2:7) = s(3:5)//s(1:3)
print *, s=='acdeabch'
end program
