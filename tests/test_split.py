"""Fortran 2023 SPLIT in interpreter and native O0/O2."""
import unittest
import test_fortran2023 as support

class SplitTests(unittest.TestCase):
    check=support.Fortran2023.check
    reject=support.Fortran2023.reject

    def test_search(self):
        self.check("""implicit none
integer :: p,i
character(6) :: text=",a,,b,"
p=0
do i=1,5
call split(text,",",p)
print *,p
end do
p=7
do i=1,5
call split(pos=p,back=.true.,set=",",string=text)
print *,p
end do
end
""", " 1\n 3\n 4\n 6\n 7\n 6\n 4\n 3\n 1\n 0\n")

    def test_expressions_and_lengths(self):
        self.check("""implicit none
integer(8)::p
logical::back
character(10)::s="ab cd"
p=0
back=.false.
call split(trim(s)//"!", " !", p, back)
print *,p
p=0
call split(s(2:5),"d",p)
print *,p
p=0
call split("",",",p)
print *,p
p=1
call split("",",",p,.true.)
print *,p
p=0
call split("abc","",p)
print *,p
end
""", " 3\n 4\n 1\n 0\n 4\n")

    def test_pure_and_array_element_output(self):
        self.check("""implicit none
integer::p(2),i
p=0
i=2
call split("abc:def",":",p(i))
print *,p
print *,delimiter("a,b")
contains
pure integer function delimiter(s) result(p)
character(*),intent(in)::s
p=0
call split(s,",",p)
end function
end
""", " 0 4\n 2\n")

    def test_integer_kinds_and_runtime_character(self):
        self.check("""implicit none
integer(1)::p1
integer(2)::p2
integer::p
logical::directions(2)=[.false.,.true.]
character(:),allocatable::s
s="a,b"
p1=0
p2=4
call split(s,",",p1)
call split(s,",",p2,directions(2))
p=0
call find("a,b",p)
print *,p1,p2,p
contains
subroutine find(s,p)
character(*),intent(in)::s
integer,intent(inout)::p
call split(s,",",p)
end subroutine
end
""", " 2 2 2\n")

    def test_evaluate_actuals_once(self):
        self.check("""implicit none
integer::calls=0,p(2),indices=0
p=0
call split(text(),",",p(index_value()),back_value())
print *,p,calls,indices
contains
function text() result(s)
character(3)::s
calls=calls+1
s="a,b"
end function
integer function index_value() result(i)
indices=indices+1
i=2
end function
logical function back_value() result(b)
calls=calls+1
b=.false.
end function
end
""", " 0 2 2 1\n")

    def test_standard_selection(self):
        import subprocess,tempfile
        from pathlib import Path
        from test_record_read import COMPILER
        with tempfile.TemporaryDirectory() as tmp:
            source=Path(tmp)/"split.f90"
            source.write_text('integer::p=0\ncall split("abc",",",p)\nend\n')
            for std,success in [("f2018",False),("f2023",True)]:
                result=subprocess.run([COMPILER,"--std",std,"check",source],capture_output=True,text=True)
                self.assertEqual(result.returncode==0,success,result.stderr)
                if not success:
                    self.assertIn("2023",result.stderr)

    def test_user_subroutine_is_preserved(self):
        self.check("""implicit none
integer::p=0
call split(p)
print *,p
contains
subroutine split(p)
integer,intent(out)::p
p=42
end subroutine
end
""", " 42\n")

    def test_invalid_arguments(self):
        for call,message in [
            ('split(1,",",p)',"STRING requires"),
            ('split("abc",1,p)',"SET requires"),
            ('split("abc",",",0)',"writable scalar INTEGER"),
            ('split("abc",",",p,1)',"BACK requires"),
            ('split("abc",",",p,unknown=.true.)',"unknown keyword"),
            ('split("abc",",")',"pos"),
        ]:
            with self.subTest(call=call):
                self.reject("implicit none\ninteger::p=0\ncall "+call+"\nend\n",message)
        self.reject('integer,parameter::p=0\ncall split("abc",",",p)\nend\n',"writable scalar INTEGER")
        self.reject('integer::p(2)=0\ncall split("abc",",",p)\nend\n',"writable scalar INTEGER")

    def test_pure_readonly_output_is_rejected(self):
        self.reject('''implicit none
contains
pure subroutine test(p)
integer,intent(in)::p
call split("abc",",",p)
end subroutine
end
''',"PURE procedure cannot modify")

    def test_invalid_position(self):
        import subprocess,tempfile
        from pathlib import Path
        from test_record_read import build_commands
        with tempfile.TemporaryDirectory() as tmp:
            directory=Path(tmp)
            source=directory/"invalid.f90"
            source.write_text('integer::p\nlogical::back\nread *,p,back\ncall split("abc",",",p,back)\nend\n')
            commands=build_commands(source,directory,cwd=directory)
            for pos,back in [(-1,"F"),(4,"F"),(0,"T"),(5,"T")]:
                for command in commands:
                    with self.subTest(pos=pos,back=back,command=command):
                        result=subprocess.run(command,input=f"{pos} {back}\n",cwd=directory,capture_output=True,text=True,timeout=30)
                        self.assertNotEqual(result.returncode,0)
                        self.assertIn("SPLIT POS is outside",result.stderr)

if __name__=="__main__":
    unittest.main()
