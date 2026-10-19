"""F2023 named ENUM types: distinct semantics from ENUMERATION TYPE."""
import subprocess
import tempfile
import unittest
from pathlib import Path

import test_fortran2023 as support
from test_record_read import COMPILER, ROOT, build_commands

ENUM = '''enum, bind(c) :: season
enumerator :: spring=5, summer=7, autumn, winter
end enum
'''


class EnumTypes(unittest.TestCase):
    check = support.Fortran2023.check
    reject = support.Fortran2023.reject

    def test_example(self):
        self.check((ROOT/'tests/programs/xenum_type.f90').read_text(),
                   ' 5 7 8 9\n 9 T\n unnamed value 6\n')

    def test_default_negative_duplicate_and_nonmonotonic_values(self):
        self.check('''implicit none
enum, bind(c) :: code
enumerator zero, one, negative=-3
enumerator next_negative, same=-3, last=100
end enum
type(code) :: c=negative
print *,zero,one,negative,next_negative,same,last,int(c)
c=code(1234)
print *,int(c)
end
''', ' 0 1 -3 -2 -3 100 -3\n 1234\n')

    def test_enumerators_are_integer_constants(self):
        self.check('implicit none\n'+ENUM+'''integer,parameter :: n=autumn+1
integer :: i
i=spring
print *,i,n,summer*2,kind(autumn)
end
''', ' 5 9 14 4\n')

    def test_assignment_and_all_comparisons(self):
        self.check('implicit none\n'+ENUM+'''type(season) :: s
s=autumn+1
print *,int(s),s==winter,winter==s,s/=summer,s>summer,s>=winter,s<spring+10,s<=winter
s=season(-10)
print *,int(s),s<spring,season(6)>spring
end
''', ' 9 T T T T T T T\n -10 T T\n')

    def test_constructor_integer_kinds_boz_and_boundaries(self):
        self.check('implicit none\n'+ENUM+'''type(season) :: s
s=season(z'ffffffff')
print *,int(s),int(season(b'101')),int(season(o'17'))
s=season(-2147483648_8)
print *,int(s,kind=8),int(season(2147483647_8),kind=8)
end
''', ' -1 5 15\n -2147483648 2147483647\n')

    def test_runtime_constructor_accepts_unnamed_values(self):
        self.check('implicit none\n'+ENUM+'''integer(8) :: n
type(season) :: s
read *,n
s=season(n)
print *,int(s)
end
''', ' -123\n', data='-123\n')

    def test_list_directed_input_and_output(self):
        self.check('implicit none\n'+ENUM+'''type(season) :: s
read *,s
print *,s
end
''', ' -123\n',data='-123\n')

    def test_runtime_type_conforming_expression_and_conditional(self):
        self.check('implicit none\n'+ENUM+'''integer :: offset
type(season) :: s
read *,offset
s=spring+offset
print *,int(s),s>summer
s=(offset>0 ? winter : spring)
print *,int(s)
end
''',' 8 T\n 9\n',data='3\n')

    def test_explicit_limits_do_not_apply_ordinal_rules(self):
        self.reject('implicit none\n'+ENUM+'''type(season) :: s
s=season(6)
print "(i3)",s
end
''','direct formatted I/O of enum objects is not yet supported')
        self.reject('implicit none\n'+ENUM+'''type(season) :: s(2)
s=[spring,summer]
end
''','type-conforming integer array expressions')

    def test_enumerator_uses_earlier_integer_parameters(self):
        self.check('''implicit none
integer,parameter :: base=10
enum,bind(c) :: code
enumerator :: first=base+1, second=first*2, third
end enum
type(code),parameter :: chosen=code(23_8)
print *,first,second,third,int(chosen)
end
''',' 11 22 23 23\n')

    def test_select_case_values_and_ranges(self):
        self.check('implicit none\n'+ENUM+'''type(season) :: s
s=season(8)
select case(s)
case(spring)
 print *,"spring"
case(summer:winter)
 print *,"summer through winter"
case default
 print *,"other"
end select
end
''', ' summer through winter\n')

    def test_named_enum_parameter_and_derived_component(self):
        self.check('implicit none\n'+ENUM+'''type(season),parameter :: chosen=summer
type box
 type(season) :: time=chosen
end type
type(box) :: b
print *,int(b%time)
b%time=winter
print *,int(b%time)
end
''', ' 7\n 9\n')

    def test_procedure_interface_and_generic_integer_distinction(self):
        self.check('module m\nimplicit none\n'+ENUM+'''interface code
module procedure enum_code,int_code
end interface
contains
integer function enum_code(s) result(i)
type(season),intent(in) :: s
integer :: i
i=100+int(s)
end function
integer function int_code(s) result(i)
integer,intent(in) :: s
integer :: i
i=200+s
end function
function identity(s) result(t)
type(season),intent(in) :: s
type(season) :: t
t=s
end function
end module
use m
implicit none
type(season) :: s
s=identity(season(6))
print *,code(s),code(spring),code(season(7))
end
''', ' 106 205 107\n')

    def test_use_renaming_and_enumerator_conformance(self):
        self.check('module m\nimplicit none\n'+ENUM+'''end module
use m,only:time=>season,fall=>autumn
implicit none
type(time) :: s
s=fall+1
print *,int(s),s>fall
end
''', ' 9 T\n')

    def test_plain_integer_is_not_type_conforming(self):
        base='implicit none\n'+ENUM+'type(season) :: s\ninteger,parameter :: n=spring\n'
        for statement in ('s=5','s=n','s=int(season(5))','print *,season(5)==5'):
            with self.subTest(statement=statement):
                self.reject(base+statement+'\nend\n','same enumeration type')

    def test_distinct_enum_and_enumeration_types(self):
        source='implicit none\n'+ENUM+'''enum,bind(c) :: colour
enumerator red=5,green
end enum
enumeration type :: level
enumerator low,high
end enumeration type
type(season) :: s
type(colour) :: c
type(level) :: l
'''
        for statement in ('s=colour(5)','s=red','s=low','l=spring','print *,season(5)==colour(5)'):
            with self.subTest(statement=statement):
                self.reject(source+statement+'\nend\n','same enumeration type')

    def test_enum_objects_do_not_have_integer_arithmetic(self):
        for expr in ('season(5)+1','-season(5)','season(5)*spring','abs(season(5))'):
            with self.subTest(expr=expr):
                self.reject('implicit none\n'+ENUM+'print *,'+expr+'\nend\n',
                            'enumeration')
        for expr in ('next(season(5))','previous(season(5))','huge(season(5))'):
            with self.subTest(expr=expr):
                self.reject('implicit none\n'+ENUM+'print *,'+expr+'\nend\n','enum type')
        self.reject('implicit none\n'+ENUM+'type(season),parameter :: s=next(season(5))\nend\n','enum type')

    def test_bad_definitions_and_constant_values(self):
        for definition,message in (
            ('enum,bind(c)::s\nenumerator a,a\nend enum','names must be distinct'),
            ('enum,bind(c)::s\nenumerator s\nend enum','names must be distinct'),
            ('enum,bind(x)::s\nenumerator a\nend enum','BIND(C)'),
            ('enum,bind(c)::s\nenumerator a=1.0\nend enum','integer constant expression'),
            ('enum,bind(c)::s\nenumerator a=b,b=1\nend enum','before its definition'),
            ('enum,bind(c)::s\nenumerator a=n\nend enum\ninteger,parameter :: n=1','before its definition'),
            ('enum,bind(c)::s\nenumerator a=2147483648_8\nend enum','C int range'),
            ('enum,bind(c)::s\nenumerator a=2147483647,b\nend enum','C int range'),
        ):
            with self.subTest(definition=definition):
                self.reject('implicit none\n'+definition+'\nend\n',message)
        self.reject(ENUM+'integer :: spring\nend\n','duplicate declaration')
        self.reject(ENUM+'spring=1\nend\n','cannot assign')

    def test_constructor_constant_range_and_type_errors(self):
        for value,message in (('2147483648_8','C int range'),('-2147483649_8','C int range'),
                              ('1.0','scalar INTEGER'),('spring,summer','scalar INTEGER')):
            with self.subTest(value=value):
                self.reject('implicit none\n'+ENUM+f'type(season)::s\ns=season({value})\nend\n',message)

    def test_runtime_constructor_range(self):
        with tempfile.TemporaryDirectory() as tmp:
            work=Path(tmp)
            path=work/'range.f90'
            path.write_text('implicit none\n'+ENUM+'''integer(8) :: n
type(season) :: s
read *,n
s=season(n)
print *,int(s)
end
''')
            for command in build_commands(path,work,cwd=work):
                for value in ('2147483648','-2147483649'):
                    result=subprocess.run(command,cwd=work,input=value+'\n',text=True,capture_output=True,timeout=30)
                    self.assertNotEqual(result.returncode,0,result.stdout+result.stderr)
                    self.assertIn('C int range',result.stderr)
                    self.assertIn('range.f90:',result.stderr)

    def test_standard_selection_and_enum_as_variable_name(self):
        self.check('implicit none\ninteger :: enum=3\nprint *,enum\nend\n',' 3\n')
        with tempfile.TemporaryDirectory() as tmp:
            work=Path(tmp);path=work/'standard.f90';path.write_text(ENUM+'end\n')
            for standard,ok in (('f2003',False),('f2018',False),('f2023',True)):
                result=subprocess.run([COMPILER,'-std='+standard,'check',path],cwd=work,text=True,capture_output=True)
                self.assertEqual(result.returncode==0,ok,result.stderr)
                if not ok:self.assertIn('named enum types',result.stderr)


if __name__ == '__main__':
    unittest.main()
