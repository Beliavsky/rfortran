"""Vector reads, captured writes, bounds and initialization across both backends."""
import json
from pathlib import Path
import subprocess
import tempfile
import unittest
import test_character_comparisons
from test_record_read import ROOT, COMPILER, build_commands


class VectorSubscripts(unittest.TestCase):
    compare = test_character_comparisons.CharacterComparisons.compare

    def test_print_vector_expressions(self):
        program = """program main
implicit none
integer::a(6),idx(3)
a=[10,20,30,40,50,60]
idx=[1,3,6]
print *,a(idx)+1
print *,-a(idx),a(idx)+a([6,3,1])
print *,sum(a(idx))
write(*,*)a(idx)+1
end
"""
        with tempfile.TemporaryDirectory(prefix="vector-output-") as tmp:
            directory = Path(tmp)
            source = directory / "main.f90"
            source.write_text(program, encoding="utf-8")
            self.compare(source, "11 31 61\n-10 -30 -60 70 60 70\n100\n11 31 61\n", directory)

    def test_reads_and_overlapping_assignments(self):
        source = ROOT / "tests/117_vector_subscripts.f90"
        expected = json.loads((ROOT / "tests/expectations.json").read_text(encoding="utf-8"))["cases"][source.stem]["program_stdout"]
        with tempfile.TemporaryDirectory(prefix="vector-fixture-") as tmp:
            self.compare(source, expected, Path(tmp))

    def test_loop_reevaluation_scalar_capture_nested_selection_and_inquiries(self):
        program = """program main
implicit none
integer::a(4),idx(2),picked(2),i,missing(4),unset_indices(2)
integer(8)::wide(3),wide_idx(2)
a=[10,20,30,40]
idx=[4,2]
picked=a(idx+offset())
print *,picked
print *,a(idx([2,1]))
print *,size(missing(unset_indices)),kind(missing(unset_indices)), &
        lbound(missing(unset_indices)),ubound(missing(unset_indices))
i=0
do while(sum(a(idx)) < 80)
  a=a+1
  i=i+1
  cycle
end do
if(i==10) then
  print *,i,a([4,1])
end if
wide=[10000000001_8,20000000002_8,30000000003_8]
wide_idx=[3_8,1_8]
print *,wide(wide_idx)
print *,selected_sum()
contains
integer function offset()
print *, 'offset'
offset=0
end function offset
pure integer function selected_sum()
integer::local(2),positions(2)
local=[5,6]
positions=[2,1]
selected_sum=sum(local(positions))
end function selected_sum
end program main
"""
        expected = "offset\n40 20\n20 40\n2 4 1 2\n10 50 20\n30000000003 10000000001\n11\n"
        with tempfile.TemporaryDirectory(prefix="vector-scopes-") as tmp:
            directory=Path(tmp)
            source=directory/"main.f90"
            source.write_text(program,encoding="utf-8")
            self.compare(source,expected,directory)

    def test_invalid_selections(self):
        cases = [
            ("integer::a(3)","a=1\nprint *,a([1.0,2.0])","rank-one INTEGER"),
            ("integer::a(3)","print *,size(a([1.0]))","rank-one INTEGER"),
            ("integer::a(3)","a=1\nprint *,a([1,.true.])","same type and kind"),
            ("integer::a(3)","a([1,1])=0","more than once"),
            ("integer::a(3)","a([1,2])=[1,2,3]","shapes do not conform"),
            ("integer::a(3)","a=1\nprint *,a([0,1])","outside 1:3"),
            ("integer::a(3)","a=1\nprint *,a([4])","outside 1:3"),
            ("integer::a(3),idx(2,2)","a=1\nidx=1\nprint *,a(idx)","rank-one INTEGER"),
            ("integer::a(3),idx(2)","a=1\nprint *,a(idx)","uninitialized variable `idx`"),
            ("integer::a(3),idx(2)","a=1\nidx=1\nprint *,a(idx(1:3))","outside bounds 1:2"),
            ("integer::a(3),idx(2),n","a=1\nidx=[1,2]\nn=a(idx)","assigned to a scalar"),
            ("integer::a(3),idx(2)","a=1\nidx=[1,2]\nread(*,*)a(idx)","READ into vector"),
            ("integer::a(3),b(2),idx(2)","a=1\nidx=[1,2]\nwhere([.true.,.false.])\nb=a(idx)\nend where","WHERE assignments are not yet supported"),
            ("integer::a(3),idx(2)","a=1\nidx=[1,2]\nprint *,a(pack(idx,.true.))","vector subscript expression is not yet supported"),
        ]
        with tempfile.TemporaryDirectory(prefix="vector-invalid-") as tmp:
            source=Path(tmp)/"main.f90"
            for declarations,statements,error in cases:
                with self.subTest(statements=statements):
                    source.write_text("program main\nimplicit none\n"+declarations+"\n"+statements+"\nend program main\n",encoding="utf-8")
                    result=subprocess.run([COMPILER,"emit-obj",source,Path(tmp)/"bad.obj"],capture_output=True,text=True,timeout=30)
                    self.assertNotEqual(result.returncode,0,result.stdout)
                    self.assertIn(error,result.stderr)

    def test_double_values_and_integer_conversion_subscripts(self):
        program = """program main
implicit none
real(8)::a(3)
integer::idx(2)
integer(1)::small(2)
integer(2)::medium(2)
a=[1.0d0,2.0d0,3.0d0]
a([3,1])=[4.0d0,5.0d0]
print *,a([1,3])
idx=[-3,-1]
print *,a(abs(idx)),a(int([2.0,1.0]))
print *,a(modulo([5,3],3)+1)
small=[3_1,1_1]
medium=[2_2,3_2]
print *,a(small),a(medium)
end program main
"""
        with tempfile.TemporaryDirectory(prefix="vector-types-") as tmp:
            directory=Path(tmp)
            source=directory/"main.f90"
            source.write_text(program,encoding="utf-8")
            self.compare(source,"5 4\n4 5 2 5\n4 5\n4 5 2 4\n",directory)

    def check_input(self, program, inputs):
        with tempfile.TemporaryDirectory(prefix="vector-runtime-") as tmp:
            directory=Path(tmp)
            source=directory/"main.f90"
            source.write_text(program,encoding="utf-8")
            for command in build_commands(source,directory):
                for data,expected,error in inputs:
                    with self.subTest(command=command,data=data):
                        result=subprocess.run(command,input=data,capture_output=True,text=True,timeout=15)
                        if error:
                            self.assertNotEqual(result.returncode,0)
                            self.assertIn(error,result.stderr)
                        else:
                            self.assertEqual(result.returncode,0,result.stderr)
                            self.assertEqual(result.stderr,"")
                            self.assertEqual(result.stdout.split(),expected.split())

    def test_dynamic_duplicates_and_bounds(self):
        self.check_input("""program main
integer::a(3),idx(2),j
a=[10,20,30]
read(*,*)j
idx=[1,j]
a(idx)=0
print *,a
end program main
""",[("2\n","0 0 30",None),("1\n",None,"selects an element more than once"),("0\n",None,"bounds"),("4\n",None,"bounds")])

    def test_only_selected_initialized_elements_are_read(self):
        self.check_input("""program main
integer::a(3),idx(2),j
a(1)=10
read(*,*)j
idx=[1,j]
print *,a(idx)
end program main
""",[("1\n","10 10",None),("2\n",None,"uninitialized variable `a`")])

    def test_constructor_of_default_integer_expressions(self):
        """`[i, i+1]` is a default INTEGER constructor in a WRITE with other
        items, also when the program contains a reduction."""
        program = """program main
implicit none
integer, parameter :: dp = kind(1.0d0)
real(dp) :: x(3)
integer :: i
x = [0.0_dp, 1.0_dp, 2.0_dp]
i = 1
print '(f6.2)', dot_product(x, x)
write (*,"(2f10.4,1x,i10)") x([i,i+1]), 7
write (*,"(2f10.4)") x([i+1,2*i+1])
end program main
"""
        with tempfile.TemporaryDirectory(prefix="vector-constructor-") as tmp:
            directory = Path(tmp)
            source = directory / "constructor.f90"
            source.write_text(program, encoding="utf-8")
            self.compare(source, "  5.00\n    0.0000    1.0000          7\n    1.0000    2.0000\n", directory)


if __name__ == "__main__":
    unittest.main(verbosity=2)
