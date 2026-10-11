"""Labelled executable statements and label validation across program units."""
import unittest

import test_deallocate
from test_record_read import ROOT


class StatementLabels(unittest.TestCase):
    check = test_deallocate.Deallocate.check
    reject = test_deallocate.Deallocate.reject

    def test_fixture(self):
        self.check((ROOT / "tests/132_statement_labels.f90").read_text(
            encoding="utf-8"), "sum 6\nblock\nhello\n")

    def test_unnamed_program_and_continued_labelled_write(self):
        self.check("""10 continue
20 write & ! continuation
 & (*,*) 7
30 if (.true.) write(*,*) 8; 40 continue
end
""", "7\n8\n")

    def test_labelled_empty_read_and_file_controls(self):
        self.check("""program main
integer::unit
character(len=4)::text
10 open(newunit=unit,status='scratch')
20 write(unit,'(a)') 'skip'
30 write(unit,'(a)') 'kept'
40 rewind(unit)
50 read(unit,*)
60 read(unit,'(a)') text
70 print *,text
80 close(unit)
end program main
""", "kept\n")

    def test_continue_identifier_remains_available(self):
        self.check("""program main
integer::continue
continue=7
10 continue
print *,continue
end program main
""", "7\n")

    def test_invalid_and_duplicate_labels(self):
        for source, message in [
            ("0 continue", "statement label must be between 1 and 99999"),
            ("100000 continue", "one to five decimal digits"),
            ("10 continue\n00010 continue", "duplicate statement label 10"),
            ("10 continue\nblock\n10 continue\nend block", "duplicate statement label 10"),
        ]:
            with self.subTest(source=source):
                self.reject(f"program main\n{source}\nend program main", message)

    def test_missing_and_nonliteral_io_targets(self):
        for control, target, tail, message in [
            ("err", "99", "", "undefined statement label 99"),
            ("ERR", "99", "", "undefined statement label 99"),
            ("err", "10+1", "11 continue", "decimal integer literal"),
            ("err", "0", "", "statement label must be between 1 and 99999"),
        ]:
            self.reject(f"""program main
open(20,file='missing.txt',status='old',action='read',{control}={target})
{tail}
end program main
""", message)

    def test_labels_are_local_to_each_procedure(self):
        self.reject("""program main
10 continue
contains
subroutine bad()
open(20,file='missing.txt',status='old',action='read',err=10)
end subroutine bad
end program main
""", "undefined statement label 10")

    def test_err_branch_is_not_silently_ignored(self):
        self.reject("""program main
open(20,file='missing.txt',status='old',action='read',err=1234)
1234 write(*,*) 'handler'
end program main
""", "unsupported I/O control `err`")


if __name__ == "__main__":
    unittest.main(verbosity=2)
