program character_arrays
implicit none
character(4)::words(4)
character(2)::small(3),empty(0)
words=['aaaa','bbbb','cccc','dddd']
words(2:4)=words(1:3)
print *,all(words==['aaaa','aaaa','bbbb','cccc'])
words=['aaaa','bbbb','cccc','dddd']
words(1:3)=words(2:4)
print *,all(words==['bbbb','cccc','dddd','dddd'])
words=['abcd','efgh','ijkl','mnop']
words(:)(2:4)=words(:)(1:3)
print *,all(words==['aabc','eefg','iijk','mmno'])
words=['aaaa','bbbb','cccc','dddd']
words=words(4:1:-1)
print *,all(words==['dddd','cccc','bbbb','aaaa'])
words(1:4:2)='X'
print *,all(words==['X   ','cccc','X   ','aaaa'])
words(2)(2:3)='Y'
print *,words(2)=='cY c',words(2)(2:3)=='Y '
small='z'
print *,all(small=='z'),any(small/='z'),size(small),len(small)
small=['a ','b ','c ']
small(2:1)='ignored'
print *,all(small==['a ','b ','c '])
print *,all(empty=='a'),any(empty=='a'),size(empty),len(empty)
print *,small
end program
