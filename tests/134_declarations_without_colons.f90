implicit none
character (len=8) ampm
integer (kind=4) values(8), d
real (kind=8) x
logical flag
character(3) words(2)
double precision total
ampm='AM'
values=2
d=3
x=1.5_8
flag=.true.
words=['one','two']
total=2d0
print *,len(ampm),trim(ampm),d,sum(values),x,flag
print *,words(1),total,words(2)
end
