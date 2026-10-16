"""Content-dependent CHARACTER result lengths and internal WRITE widths."""
import unittest
import test_deallocate
from test_record_read import ROOT

JOIN='''function join(words,sep) result(str)
character(*),intent(in)::words(:),sep
character((size(words)-1)*len(sep)+sum(len_trim(words)))::str
integer::i,nw
nw=size(words)
str=''
if(nw<1)return
write(str,'(10000(a))') trim(words(1)),(sep//trim(words(i)),i=2,nw)
end function
'''

class Joins(unittest.TestCase):
    check=test_deallocate.Deallocate.check

    def test_reproducer(self):
        self.check((ROOT/'xjoin_length.f90').read_text(),'4 [a:bb]\n5 [abc:d]\n')

    def test_empty_blank_and_zero_length(self):
        self.check('''character(3)::empty(0),blank(2)
character(0)::zero(2)
blank=''
print *,len(join(empty,':')),'['//join(empty,':')//']'
print *,len(join(blank,':')),'['//join(blank,':')//']'
print *,len(join(zero,':')),'['//join(zero,':')//']'
contains
'''+JOIN+'end\n','0 []\n1 [:]\n1 [:]\n')

    def test_runtime_contents(self):
        self.check('''character(5)::words(3)
read(*,'(a)')words(1)
read(*,'(a)')words(2)
read(*,'(a)')words(3)
print *,len(join(words,'--')),'['//join(words,'--')//']'
contains
'''+JOIN+'end\n','7 [ab----c]\n',data='ab\n\nc\n')

    def test_runtime_section(self):
        source='''character(4)::words(4)
integer::n
words(1)='ab'
words(2)='c'
! The unused capacity remains uninitialized.
read(*,*)n
print *,len(join(words(:n),':')),'['//join(words(:n),':')//']'
contains
'''+JOIN+'end\n'
        for n,expected in [(2,'4 [ab:c]\n'),(0,'0 []\n')]:
            with self.subTest(n=n):self.check(source,expected,data=str(n)+'\n')

    def test_internal_write_uses_declared_runtime_width(self):
        self.check('''character(5)::text
text='ab'
print *,bad(text)
contains
function bad(text) result(out)
character(*),intent(in)::text
character(len_trim(text))::out
write(out,'(a)') '123'
end function
end
''',error='internal WRITE record exceeds CHARACTER destination length',reference=False)

    def test_fixed_entry_length_and_local_storage(self):
        self.check('''character(5)::text,result
text='abc'
result=take(text)
print *,'['//trim(result)//']',text==''
contains
function take(text) result(out)
character(*),intent(inout)::text
character(len_trim(text))::out,local
text=''
local='12345'
if(len(local)/=3)stop 1
out=local
if(len(out)/=3)stop 2
end function
end
''','[123] T\n')

    def test_result_as_assumed_length_argument(self):
        self.check('''character(4)::words(2)
words=['a   ','bb  ']
print *,inspect(join(words,':'))
contains
integer function inspect(text)
character(*),intent(in)::text
inspect=len(text)
end function
'''+JOIN+'end\n','4\n')

    def test_result_as_character_argument(self):
        self.check('''character(4)::words(2)
words=['a   ','bb  ']
print *,len(wrap(join(words,':'))),'['//wrap(join(words,':'))//']'
contains
function wrap(text) result(out)
character(*),intent(in)::text
character(len(text)+2)::out
out='['//text//']'
end function
'''+JOIN+'end\n','6 [[a:bb]]\n')

    def test_fixed_dummy_keeps_declared_length(self):
        self.check('''character(4)::words(2)
words=['a   ','bb  ']
print *,len(prefix(join(words,':'))),'['//prefix(join(words,':'))//']'
contains
function prefix(text) result(out)
character(3),intent(in)::text
character(len(text))::out
out=text
end function
'''+JOIN+'end\n','3 [a:b]\n')

if __name__=='__main__':unittest.main(verbosity=2)
