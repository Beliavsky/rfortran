"""Empty list-directed READ in the interpreter and native O0/O2 code."""
import unittest

import test_deallocate
from test_record_read import ROOT


class EmptyRead(unittest.TestCase):
    check = test_deallocate.Deallocate.check
    reject = test_deallocate.Deallocate.reject

    def test_scratch_file_positioning_and_status(self):
        self.check((ROOT / "tests/131_empty_read.f90").read_text(
            encoding="utf-8"), "kept\nT\nT\nT\nT\n")

    def test_standard_input_blank_long_and_uninterpreted_records(self):
        program = """program main
integer::iunit,ios
character(len=5)::text
iunit=5
read(*,*)
read(iunit,*,iostat=ios)
print *,ios
if (.true.) read(5,*); print *,'ready'
read(*,'(a)') text
print *, '['//text//']'
read(*,*,iostat=ios)
print *,ios<0
end program main
"""
        data = "not numeric, / ' \u00e9\x00" * 1200 + "\n\nignored\nlast\n"
        self.check(program, "0\nready\n[last ]\nT\n", data=data)
        self.check(program, "0\nready\n[last ]\nT\n", data=data.replace("\n", "\r\n"))

    def test_partial_final_record_then_eof(self):
        program = """program main
integer::ios
read(*,*,iostat=ios)
print *,ios==0
read(*,*,iostat=ios)
print *,ios<0
end program main
"""
        self.check(program, "T\nT\n", data="final record without newline")

    def test_eof_without_iostat_is_an_error(self):
        program = "program main\nread(*,*)\nend program main"
        self.check(program, error="end of file or input error in empty READ")

    def test_unconnected_unit_without_iostat(self):
        self.check("program main\nread(23,*)\nend program main",
                   error="end of file or input error in empty READ")

    def test_continuations_and_following_statements(self):
        self.check("""program main
character(len=2)::source
character(len=4)::text
integer::n
source='42'
read(source,*) & ! input list continues
! intervening comment
 & n
print *,n
read & ! comment
 & (5,*) ! discard one record
read(*,'(a)') text
print *,text
end program main
""", "42\nkept\n", data="discarded\nkept\n")

    def test_specialized_subroutine_with_output_status(self):
        self.check("""program main
integer::values(2)
call discard(5,values)
print *,values
call discard(5,values)
print *,all(values<0)
contains
subroutine discard(unit,values)
integer,intent(in)::unit
integer,intent(out)::values(:)
integer::ios
read(unit,*,iostat=ios)
values=ios
end subroutine discard
end program main
""", "0 0\nT\n", data="discarded\n")

    def test_formatted_skips_and_format_controls(self):
        self.check("""implicit none
integer :: u,ios
character(8) :: text
open(newunit=u,status='scratch',form='formatted')
write(u,'(a)') 'header','one','two','three','four','five','six'
rewind(u)
read(u,'(a)')
read(u,'(a)') text
print *,trim(text)
read(u,'(/)')
read(u,'(a)') text
print *,trim(text)
read(u,'(:,/)')
read(u,'(a)') text
print *,trim(text)
read(u,'(i5)',iostat=ios)
print *,ios<0
close(u)
end
""", 'one four six T')

    def test_formatted_stdin_labels_and_runtime_format(self):
        self.check("""implicit none
integer :: ios
character(8) :: fmt
character(4) :: text
fmt='(A)'
read(*,fmt,iostat=ios)
print *,ios==0
read(*,100)
100 format(a)
read(*,'(a)') text
print *,text
read(*,'(a)',end=200)
error stop 'expected EOF branch'
200 print *, 'done'
end
""", 'T kept done', data='discard\nignored\nkept\n')

    def test_empty_formatted_internal_file(self):
        self.check("""implicit none
character(5) :: line
integer :: ios
line='hello'
read(line,'(a)',iostat=ios)
print *,ios==0,line
read(line,'(/)',iostat=ios)
print *,ios==0
end
""", 'T hello T')

    def test_invalid_controls_and_pure_io(self):
        for declarations, statement, message in [
            ("integer::ios", "read(20,*,unknown=ios)", "only the IOSTAT control"),
            ("real::ios", "read(20,*,iostat=ios)", "IOSTAT must be a writable scalar INTEGER"),
            ("", "read(20,*,iostat=1)", "IOSTAT must be a writable scalar INTEGER"),
            ("character(len=3)::text", "read(text,*)", "file unit currently requires a scalar INTEGER"),
            ("real::iunit", "read(iunit,*)", "file unit currently requires a scalar INTEGER"),
        ]:
            with self.subTest(statement=statement):
                self.reject(f"program main\n{declarations}\n{statement}\nend program main", message)
        self.reject("""program main
contains
pure subroutine bad()
read(5,*)
end subroutine bad
end program main
""", "PURE procedure cannot perform external I/O")


if __name__ == "__main__":
    unittest.main(verbosity=2)
