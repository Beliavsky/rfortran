"""ELEMENTAL runtime array expressions inside reductions, in both backends."""
import unittest
import test_runtime_dim_reductions
from test_record_read import ROOT


class RuntimeElementalReductions(unittest.TestCase):
    check = test_runtime_dim_reductions.RuntimeDimReductions.check
    compare = test_runtime_dim_reductions.RuntimeDimReductions.compare

    def test_derived_comparison_fixture(self):
        program = (ROOT / "tests/programs/xacv_runtime_derived_comparison.f90").read_text()
        self.check(program, "T\n")

    def test_comparison_forms_and_logical_reductions(self):
        program = (ROOT / "tests/programs/xacv_runtime_derived_comparison.f90").read_text()
        program = program.replace("print *,all(dates(2:) < dates(:nread-1))", """
print *,all(dates(2:) < dates(:nread-1))
print *,all(date_mdy_lt(dates(2:),dates(:nread-1)))
print *,any(dates(:nread-1) < dates(2:))
print *,count(dates(2:) < dates(:nread-1),kind=8)
print *,parity(dates(2:) < dates(:nread-1))
print *,all(.not.(dates(:nread-1) < dates(2:)))
print *,all(dates(2:1) < dates(1:0))
""")
        self.check(program, "T\nT\nF\n2\nF\nT\nT\n")

    def test_numeric_results_masks_locations_and_kinds(self):
        for kind, value in [("integer(8)", "2_8"), ("real", "2."), ("double precision", "2d0")]:
            with self.subTest(kind=kind):
                self.compare(f"""program main
implicit none
{kind},allocatable::a(:,:,:)
integer::n
n=2
allocate(a(n,3,n))
a={value}
print *,sum(twice(a),dim=1),product(twice(a),dim=2)
print *,minval(twice(a),dim=3),maxval(twice(a),mask=positive(a),dim=2)
print *,count(positive(a),dim=1),any(positive(a),dim=2),all(positive(a),dim=3)
print *,minloc(twice(a),dim=2,back=.true.),maxloc(twice(a),dim=1)
print *,sum(twice(twice(a)))
contains
pure elemental function twice(x) result(y)
{kind},intent(in)::x
{kind}::y
y=2*x
end function
pure elemental logical function positive(x)
{kind},intent(in)::x
positive=x>0
end function
end program
""")

    def test_impure_elemental_and_empty_input(self):
        self.compare("""program main
implicit none
integer,allocatable::a(:)
integer::n
n=3
allocate(a(n))
a=[1,2,3]
print *,sum(twice(a)),count(positive(a)),all(positive(a))
deallocate(a)
allocate(a(0))
print *,sum(twice(a)),count(positive(a)),all(positive(a))
contains
impure elemental integer function twice(x)
integer,intent(in)::x
twice=2*x
end function
impure elemental logical function positive(x)
integer,intent(in)::x
positive=x>0
end function
end program
""")


if __name__ == "__main__":
    unittest.main()
