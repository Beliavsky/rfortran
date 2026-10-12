implicit none
character (len=2), parameter :: s(*) = ["aa", "bb", "cc"], t(3) = ["aa", "bb", "cc"]
print "(*(1x,a))", s
print "(*(1x,a))", t
end
