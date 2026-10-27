"""IEEE_VALUE molds supply type/shape without reading undefined storage."""
import os
from pathlib import Path
import tempfile
import unittest

import test_deallocate
from test_record_read import ROOT

HEADER='use, intrinsic :: ieee_arithmetic\nimplicit none\n'


class IeeeValueMold(unittest.TestCase):
    # Intrinsic-module resolution must not see module caches from unrelated
    # tests that deliberately define a user IEEE_ARITHMETIC module.
    def check(self, source, expected=None, **kwargs):
        previous=Path.cwd()
        with tempfile.TemporaryDirectory(prefix='ieee-value-work-') as tmp:
            try:
                os.chdir(tmp)
                test_deallocate.Deallocate.check(self,source,expected,**kwargs)
            finally:
                os.chdir(previous)

    def reject(self, source, message='uninitialized variable'):
        previous=Path.cwd()
        with tempfile.TemporaryDirectory(prefix='ieee-value-invalid-work-') as tmp:
            try:
                os.chdir(tmp)
                test_deallocate.Deallocate.reject(self,source,message)
            finally:
                os.chdir(previous)

    def test_scalar_reproducer(self):
        self.check((ROOT/'tests/programs/xieee_value_mold.f90').read_text(),'T')

    def test_scalar_molds_both_kinds(self):
        for kind in (4,8):
            with self.subTest(kind=kind):
                self.check(HEADER+f'''real(kind={kind})::x,y,z
y=ieee_value(x,ieee_quiet_nan)
z=ieee_value(x,ieee_positive_inf)
print *,kind(y),ieee_is_nan(y),ieee_is_finite(z),z>huge(z)
x=ieee_value(x,ieee_negative_zero)
print *,x==0
end
''',f'{kind} T F T\nT')

    def test_double_precision_constructor_retains_result_kind(self):
        self.check(HEADER+"""real(kind=8)::x,y(3)
y=[1.0_8,ieee_value(x,ieee_quiet_nan),3.0_8]
print *,kind(y),ieee_is_nan(y),sum(y([1,3]))
end
""",'8 F T F 4')

    def test_function_result_mold(self):
        self.check('''module m
use, intrinsic :: ieee_arithmetic
contains
pure function missing() result(x)
real(kind=8)::x
x=ieee_value(x,ieee_quiet_nan)
end function
end module
program p
use m
print *,ieee_is_nan(missing())
end
''','T')

    def test_fixed_multidimensional_array_mold(self):
        self.check(HEADER+'''real(kind=8)::x(2,3),y(2,3)
y=ieee_value(x,ieee_quiet_nan)
print *,shape(y),all(ieee_is_nan(y))
x=ieee_value(x,ieee_positive_inf)
print *,all(.not.ieee_is_finite(x))
end
''','2 3 T\nT')

    def test_sections_and_nondefault_bounds(self):
        self.check(HEADER+'''real::x(-2:3),y(3)
y=ieee_value(x(-2:3:2),ieee_quiet_nan)
print *,size(y),all(ieee_is_nan(y))
end
''','3 T')

    def test_runtime_allocated_multidimensional_mold(self):
        self.check(HEADER+'''integer::n
real(kind=8),allocatable::x(:,:),y(:,:)
read *,n
allocate(x(n,2),y(n,2))
y=ieee_value(x,ieee_quiet_nan)
print *,shape(y),all(ieee_is_nan(y))
end
''','3 2 T',data='3\n')

    def test_zero_size_runtime_mold(self):
        self.check(HEADER+"""integer::n
real,allocatable::x(:,:),y(:,:)
read *,n
allocate(x(n,2),y(n,2))
y=ieee_value(x,ieee_quiet_nan)
print *,shape(y),all(ieee_is_nan(y))
end
""",'0 2 T',data='0\n')

    def test_equal_size_different_shapes_are_rejected(self):
        self.reject(HEADER+"""real::x(2,3),y(2,3)
type(ieee_class_type)::classes(3,2)
classes=ieee_quiet_nan
y=ieee_value(x,classes)
end
""",'conform')

    def test_runtime_mold_expression_as_procedure_actual(self):
        source=HEADER+"""integer::n
real,allocatable::x(:)
read *,n
allocate(x(n))
call show(ieee_value(x,ieee_quiet_nan))
contains
subroutine show(a)
real,intent(in)::a(:)
print *,size(a),all(ieee_is_nan(a))
end subroutine
end
"""
        for n in (0,3):
            with self.subTest(n=n):
                self.check(source,f'{n} T',data=f'{n}\n')

    def test_scalar_mold_and_array_classes(self):
        self.check(HEADER+'''real::x,y(2)
y=ieee_value(x,[ieee_quiet_nan,ieee_positive_inf])
print *,ieee_is_nan(y),ieee_is_finite(y)
end
''','T F F F')

    def test_renamed_reexported_intrinsic_and_keywords(self):
        self.check('''module m
use,intrinsic::ieee_arithmetic,only:value=>ieee_value,ieee_quiet_nan,ieee_is_nan
end module
program p
use m,only:make=>value,ieee_quiet_nan,ieee_is_nan
real(kind=8)::x,y
y=make(class=ieee_quiet_nan,x=x)
print *,kind(y),ieee_is_nan(y)
end
''','8 T')

    def test_mold_is_not_marked_initialized(self):
        self.reject(HEADER+'real::x,y\ny=ieee_value(x,ieee_quiet_nan)\nprint *,x\nend\n')

    def test_real_value_inputs_still_require_initialization(self):
        for expression in ['ieee_is_nan(x)','scale(x,1)','ieee_is_finite(x)']:
            with self.subTest(expression=expression):
                self.reject(HEADER+'real::x\nprint *,'+expression+'\nend\n')

    def test_class_value_still_requires_initialization(self):
        self.reject(HEADER+'real::x\ntype(ieee_class_type)::c\nx=ieee_value(x,c)\nend\n')

    def test_maybe_initialized_mold_does_not_trigger_runtime_guard(self):
        self.check(HEADER+'''real::x,y
integer::i
read *,i
if(i>0)x=1
y=ieee_value(x,ieee_quiet_nan)
print *,ieee_is_nan(y)
end
''','T',data='0\n')

    def test_real_read_after_mold_still_triggers_runtime_guard(self):
        self.check(HEADER+'''real::x,y
integer::i
read *,i
if(i>0)x=1
y=ieee_value(x,ieee_quiet_nan)
print *,ieee_is_nan(y)
print *,x
end
''',error='uninitialized variable `x`',data='0\n',reference=False)

    def test_unrelated_user_procedure_not_treated_as_intrinsic(self):
        self.reject('''module custom
contains
real function ieee_value4(x)
real,intent(in)::x
ieee_value4=x
end function
end module
program p
use custom,only:ieee_value=>ieee_value4
real::x
print *,ieee_value(x)
end
''')


if __name__=='__main__':
    unittest.main()
