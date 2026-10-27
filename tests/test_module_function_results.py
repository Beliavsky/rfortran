"""Function-name results must retain their identity after module association."""
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

import test_deallocate
from test_record_read import ROOT, COMPILER

PREFIX = '''module factories
implicit none
type pair
integer :: first,second
end type
contains
function make_pair(n)
integer,intent(in)::n
type(pair)::make_pair
make_pair%first=n
make_pair%second=n+1
end function
end module
'''


class ModuleFunctionResults(unittest.TestCase):
    check = test_deallocate.Deallocate.check

    def test_private_constructor_reproducer(self):
        self.check((ROOT/'tests/programs/xprivate_constructor_result.f90').read_text(),'7')

    def test_public_factory_and_keyword_arguments(self):
        self.check(PREFIX+'''program p
use factories
implicit none
type(pair)::x
x=make_pair(n=4)
print *,x%first,x%second
end
''','4 5')

    def test_typed_function_header(self):
        source=PREFIX.replace('function make_pair(n)','type(pair) function make_pair(n)').replace('type(pair)::make_pair\n','')
        self.check(source+'program p\nuse factories\ntype(pair)::x\nx=make_pair(6)\nprint *,x%first,x%second\nend\n','6 7')

    def test_renamed_import_and_reexport(self):
        self.check(PREFIX+'''module bridge
use factories,only:pair,build=>make_pair
end module
program p
use bridge,only:pair,create=>build
type(pair)::x
x=create(8)
print *,x%first,x%second
end
''','8 9')

    def test_forward_result_through_another_function(self):
        source=PREFIX.replace('end module','''function forward(n)
integer::n
type(pair)::forward
forward=make_pair(n)
end function
end module''')
        self.check(source+'program p\nuse factories\ntype(pair)::x\nx=forward(10)\nprint *,x%first,x%second\nend\n','10 11')

    def test_explicit_result_remains_local(self):
        source=PREFIX.replace('function make_pair(n)','function make_pair(n) result(x)').replace('type(pair)::make_pair','type(pair)::x').replace('make_pair%','x%')
        self.check(source+'program p\nuse factories\ntype(pair)::x\nx=make_pair(12)\nprint *,x%first,x%second\nend\n','12 13')

    def test_intrinsic_array_and_character_function_name_results(self):
        self.check('''module functions
contains
function indices(n)
integer,intent(in)::n
integer::indices(2)
indices=[n,n+1]
end function
function label()
character(len=3)::label
label='abc'
end function
end module
program p
use functions
print *,indices(14)
print '(a)',label()
end
''','14 15\nabc')

    def test_module_file_round_trip(self):
        with tempfile.TemporaryDirectory(prefix='function-result-rmod-') as tmp:
            directory=Path(tmp);source=directory/'factories.f90';source.write_text(PREFIX)
            main=directory/'main.f90';main.write_text('program p\nuse factories\ntype(pair)::x\nx=make_pair(16)\nprint *,x%first,x%second\nend\n')
            def run(args):
                result=subprocess.run(list(map(str,args)),cwd=directory,capture_output=True,text=True,timeout=60)
                self.assertEqual(result.returncode,0,result.stderr)
                return result
            run([COMPILER,'-c',source,'-o',directory/'factories.obj'])
            obj=directory/'main.obj'
            run([COMPILER,'-c',main,'-o',obj])
            exe=directory/('program.exe' if os.name=='nt' else 'program')
            run([COMPILER,'link',obj,directory/'factories.obj','-o',exe])
            self.assertEqual(run([exe]).stdout.split(),['16','17'])


if __name__=='__main__':
    unittest.main()
