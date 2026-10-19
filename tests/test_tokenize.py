"""TOKENIZE overloads in the interpreter and native O0/O2."""
import unittest
import test_fortran2023 as support

class Tokenize(unittest.TestCase):
    check=support.Fortran2023.check
    reject=support.Fortran2023.reject

    def test_boundaries(self):
        self.check("""implicit none
integer,allocatable::first(:),last(:)
call tokenize("a,b,,c,",",",first,last)
print *,first
print *,last
print *,lbound(first),lbound(last)
call tokenize(string="word",set="",last=last,first=first)
print *,first,last
call tokenize("",",",first,last)
print *,first,last
end
""", " 1 3 5 6 8\n 1 3 4 6 7\n 1 1\n 1 4\n 1 0\n")

    def test_tokens_and_separators(self):
        self.check("""implicit none
character(:),allocatable::words(:),seps(:)
integer::i
call tokenize("a,b,,ccc,",",",words,seps)
print *,size(words),len(words),size(seps),len(seps)
do i=1,size(words)
print "(a,a,a)","[",words(i),"]"
end do
print "(*(a))",seps
print *,len(words(1))
end
""", " 5 3 4 1\n[a  ]\n[b  ]\n[   ]\n[ccc]\n[   ]\n,,,,\n 3\n")

    def test_character_reallocation_and_empty_results(self):
        self.check("""implicit none
character(:),allocatable::t(:),s(:)
call tokenize(tokens=t,separator=s,string="one:longer|x",set=":|")
print *,size(t),len(t),size(s),len(s)
print "('[',a,']')",t
print "(*(a))",s
call tokenize("",",",t,s)
print *,allocated(t),allocated(s),size(t),len(t),size(s),len(s)
print "('[',a,']')",t(1)
call tokenize(",,",",",t,s)
print *,size(t),len(t),size(s),len(s)
print "('[',a,']')",t
print "(*(a))",s
call tokenize("abc","",t)
print *,size(t),len(t)
print "('[',a,']')",t
end
""", " 3 6 2 1\n[one   ]\n[longer]\n[x     ]\n:|\n T T 1 0 0 1\n[]\n 3 0 2 1\n[]\n[]\n[]\n,,\n 1 3\n[abc]\n")

    def test_input_expressions_are_evaluated_once(self):
        self.check("""implicit none
integer::calls=0
integer,allocatable::f(:),l(:)
call tokenize(text(),delimiters(),f,l)
print *,f,l,calls
contains
function text() result(s)
character(3)::s
calls=calls+1
s="a,b"
end function
function delimiters() result(s)
character(1)::s
calls=calls+1
s=","
end function
end
""", " 1 3 1 3 2\n")

    def test_integer_kinds_and_pure_call(self):
        self.check("""implicit none
integer(1),allocatable::f(:)
integer(8),allocatable::l(:)
call parts("a,b",f,l)
print *,f,l
contains
pure subroutine parts(s,f,l)
character(*),intent(in)::s
integer(1),allocatable,intent(out)::f(:)
integer(8),allocatable,intent(out)::l(:)
call tokenize(first=f,last=l,string=s,set=",")
end subroutine
end
""", " 1 3 1 3\n")

    def test_pure_character_outputs(self):
        self.check("""implicit none
character(:),allocatable::t(:),s(:)
call parts("one,two",t,s)
print *,len(t),size(t),len(s),size(s)
print "('[',a,']')",t
call parts("longer:x",t,s)
print *,len(t),size(t),len(s),size(s)
print "('[',a,']')",t
contains
pure subroutine parts(text,t,s)
character(*),intent(in)::text
character(:),allocatable,intent(out)::t(:),s(:)
call tokenize(text,",:",t,s)
end subroutine
end
""", " 3 2 1 1\n[one]\n[two]\n 6 2 1 1\n[longer]\n[x     ]\n")

    def test_long_strings_and_blanks(self):
        self.check("""implicit none
character(:),allocatable::t(:)
character(5)::text="a b  "
call tokenize(repeat("x",5000),",",t)
print *,size(t),len(t),len_trim(t(1))
call tokenize(text," ",t)
print *,size(t),len(t)
print "('[',a,']')",t
end
""", " 1 5000 5000\n 4 1\n[a]\n[b]\n[ ]\n[ ]\n")

    def test_user_procedure_and_select_case(self):
        self.check("""implicit none
integer,allocatable::a(:),b(:)
integer::i=1
select case(i)
case(1)
call tokenize("a,b",",",a,b)
end select
print *,a,b
end
""", " 1 3 1 3\n")
        self.check("""implicit none
integer::x
call tokenize(x)
print *,x
contains
subroutine tokenize(x)
integer,intent(out)::x
x=42
end subroutine
end
""", " 42\n")

    def test_module_user_name_does_not_hide_intrinsic(self):
        self.check("""module m
implicit none
contains
subroutine tokenize(x)
integer,intent(out)::x
x=42
end subroutine
end module
use m,only:custom=>tokenize
implicit none
integer::x
integer,allocatable::f(:),l(:)
call custom(x)
call tokenize("a,b",",",f,l)
print *,x,f,l
end
""", " 42 1 3 1 3\n")

    def test_standard_and_keyword_diagnostics(self):
        import tempfile,subprocess
        from pathlib import Path
        from test_record_read import COMPILER
        with tempfile.TemporaryDirectory() as tmp:
            source=Path(tmp)/"tokenize.f90"
            source.write_text('integer,allocatable::a(:),b(:)\ncall tokenize("a",",",a,b)\nend\n')
            for std,success in [("f2018",False),("f2023",True)]:
                result=subprocess.run([COMPILER,"--std",std,"check",source],capture_output=True,text=True)
                self.assertEqual(result.returncode==0,success,result.stderr)
                if not success:self.assertIn("2023",result.stderr)
        self.reject('integer,allocatable::a(:),b(:)\ncall tokenize("a",",",first=a,last=b,set=",")\nend\n',"duplicate argument")

    def test_assumed_length_array_input(self):
        self.check("""implicit none
character(:),allocatable::t(:)
call tokenize("a,bbb",",",t)
call show(t)
contains
subroutine show(words)
character(*),intent(in)::words(:)
print *,len(words),size(words)
print "('[',a,']')",words
end subroutine
end
""", " 3 2\n[a  ]\n[bbb]\n")

    def test_small_integer_overflow(self):
        self.reject('integer(1),allocatable::f(:),l(:)\ncall tokenize(repeat("x",128),",",f,l)\nend\n',"TOKENIZE LAST result cannot be represented",modes=("run",))

    def test_invalid_arguments(self):
        cases=[
            ('integer,allocatable::a(:),b(:)','tokenize(1,",",a,b)',"STRING requires"),
            ('integer,allocatable::a(:),b(:)','tokenize("a",1,a,b)',"SET requires"),
            ('integer::a(2),b(2)','tokenize("a",",",a,b)',"allocatable rank-one"),
            ('integer,allocatable::a(:,:),b(:)','tokenize("a",",",a,b)',"rank-one"),
            ('real,allocatable::a(:),b(:)','tokenize("a",",",a,b)',"requires INTEGER"),
            ('character(3),allocatable::a(:)','tokenize("a",",",a)',"deferred-length CHARACTER"),
            ('integer,allocatable::a(:),b(:)','tokenize("a",",",first=a)',"last"),
            ('character(:),allocatable::a(:),b(:)','tokenize("a",",",tokens=a,last=b)',"cannot mix"),
            ('integer,allocatable::a(:),b(:)','tokenize("a",",",a,b,unknown=1)',"unknown keyword"),
            ('integer,allocatable::a(:)','tokenize("a",",",a,a)',"must be distinct"),
            ('character(:),allocatable::a(:),b(:)','tokenize("a",",",first=a,last=b)',"unknown keyword"),
            ('integer,allocatable::a(:),b(:)','tokenize("a",",",tokens=a,separator=b)',"unknown keyword"),
        ]
        for declaration,call,message in cases:
            with self.subTest(call=call):self.reject(f"implicit none\n{declaration}\ncall {call}\nend\n",message)

if __name__=="__main__":
    unittest.main()
