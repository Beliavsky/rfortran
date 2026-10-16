"""INQUIRE connection state in the interpreter, native O0/O2, and gfortran."""
import tempfile
from pathlib import Path
import unittest
import test_deallocate
from test_file_io import literal
from test_record_read import ROOT


class Inquire(unittest.TestCase):
    check=test_deallocate.Deallocate.check
    reject=test_deallocate.Deallocate.reject

    def test_unit_states(self):
        self.check((ROOT/'tests/198_inquire.f90').read_text(),'inquire tests passed\n')

    def test_file_name_and_connection_transitions(self):
        with tempfile.TemporaryDirectory(prefix='inquire file (A&B) ') as tmp:
            path=Path(tmp)/'records.txt';path.write_text('hello\n')
            self.check(f'''character(512)::path,name
character(3)::short
logical::opened
integer::number,ios
path={literal(path)}
inquire(file=path,opened=opened,number=number,iostat=ios)
print *,opened,number,ios
open(20,file={literal(path)},status='old',action='read')
inquire(name=name,opened=opened,file=trim(path),iostat=ios,number=number)
print *,opened,number,ios,trim(name)==trim(path)
inquire(unit=20,name=short)
print *,short==path(1:3)
inquire(file=''//trim(path),name=path,number=number,opened=opened)
print *,opened,number,trim(path)==trim(name)
close(20)
inquire(file=path,opened=opened,number=number,iostat=ios)
print *,opened,number,ios
inquire(file={literal(Path(tmp)/'missing.txt')},opened=opened,number=number,iostat=ios)
print *,opened,number,ios
end
''','F -1 0\nT 20 0 T\nT\nT 20 T\nF -1 0\nF -1 0\n')

    def test_normalized_file_names(self):
        with tempfile.TemporaryDirectory(prefix='inquire paths ') as tmp:
            directory=Path(tmp);(directory/'sub').mkdir()
            path=directory/'records.txt';path.write_text('hello\n')
            alias=directory/'sub'/'..'/'records.txt'
            self.check(f'''integer::number,ios
logical::opened
open(21,file={literal(path)},status='old',action='read')
inquire(file={literal(alias)},opened=opened,number=number,iostat=ios)
print *,opened,number,ios
close(21)
end
''','T 21 0\n')

    def test_outputs_in_subroutine_and_get_unit(self):
        self.check('''program main
integer::u,other
call get_unit(u)
print *,u
open(20,status='scratch')
call get_unit(other,'missing-inquiry-file')
print *,other
close(20)
call get_unit(u)
print *,u
contains
subroutine get_unit(iunit,xfile)
integer,intent(out)::iunit
character(*),intent(in),optional::xfile
integer::i,ios
logical::opened
if (present(xfile)) then
   inquire(file=xfile,number=iunit)
   if (iunit/=-1) return
end if
iunit=0
do i=20,99
   inquire(unit=i,opened=opened,iostat=ios)
   if (ios==0) then
      if (.not.opened) then
         iunit=i
         return
      end if
   end if
end do
end subroutine get_unit
end program main
''','20\n21\n20\n')

    def test_integer_kinds_and_unit_expression(self):
        self.check('''integer,parameter::small=selected_int_kind(2),wide=selected_int_kind(18)
integer(small)::n,ios
integer(wide)::u
logical::opened
u=20
inquire(unit=u+1,opened=opened,number=n,iostat=ios)
print *,opened,n,ios
open(21,status='scratch')
inquire(unit=u+1,opened=opened,number=n,iostat=ios)
print *,opened,n,ios
close(21)
end
''','F -1 0\nT 21 0\n')

    def test_invalid_unit_without_iostat(self):
        self.check('logical::opened\ninquire(unit=-1,opened=opened)\nend\n',
                   error='INQUIRE failed',reference=False)

    def test_validation(self):
        for controls,message in [
            ('opened=opened','exactly one'),
            ("unit=20,file='x',opened=opened",'exactly one'),
            ('unit=20,unit=21,opened=opened','duplicate unit'),
            ('unit=20,opened=1','writable scalar'),
            ('unit=20,opened=n','LOGICAL'),
            ('unit=20,number=opened','INTEGER'),
            ('unit=20,name=n','CHARACTER'),
            ('unit=20,iostat=opened','INTEGER'),
            ('unit=1.0,opened=opened','INTEGER'),
            ('file=20,opened=opened','CHARACTER'),
            ('unit=20,iostat=n(1)','writable scalar'),
        ]:
            with self.subTest(controls=controls):
                self.reject('integer::n\nlogical::opened\ninquire('+controls+')\nend\n',message)
        self.reject('pure subroutine f()\nlogical::opened\ninquire(unit=20,opened=opened)\nend subroutine\n',
                    'PURE procedure cannot perform external I/O')


if __name__=='__main__':unittest.main()
