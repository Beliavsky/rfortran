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

    def test_xacv_file_exists(self):
        with tempfile.TemporaryDirectory(prefix='inquire existence ') as tmp:
            program=(ROOT/'tests/programs/xacv_inquire_exist.f90').read_text()
            program=program.replace("'rfort_inquire_exist_test.tmp'",literal(Path(tmp)/'exist.tmp'))
            self.check(program,'T\nF\n')

    def test_existence_named_and_action(self):
        with tempfile.TemporaryDirectory(prefix='inquire properties ') as tmp:
            path=Path(tmp)/'data.txt'
            self.check(f'''implicit none
character(512)::path,name
character(12)::action
character(4)::short
logical::exists,opened,named
integer::u,ios
path={literal(path)}
inquire(file=path,exist=exists,opened=opened,named=named,name=name,action=action)
print *,exists,opened,named,trim(name)==trim(path),trim(action)=='UNDEFINED'
open(newunit=u,file=path,status='replace',action='write')
inquire(unit=u,exist=exists,opened=opened,named=named,name=name,action=action,iostat=ios)
print *,exists,opened,named,trim(name)==trim(path),trim(action)=='WRITE',ios
inquire(file=path,action=short)
print *,short=='WRIT'
close(u)
inquire(file=path,exist=exists,opened=opened,named=named)
print *,exists,opened,named
open(20,file=path,status='old',action='read')
inquire(file=path,action=action)
print *,trim(action)=='READ'
close(20)
open(20,file=path,status='old',action='readwrite')
inquire(unit=20,action=action)
print *,trim(action)=='READWRITE'
close(20)
open(20,file=path,status='unknown',action='write')
inquire(unit=20,action=action)
print *,trim(action)=='WRITE'
close(20,status='delete')
inquire(file=path,exist=exists)
print *,exists
open(20,file=path,status='unknown',action='write')
inquire(file=path,exist=exists,action=action)
print *,exists,trim(action)=='WRITE'
close(20,status='delete')
end
''','F F T T T\nT T T T T 0\nT\nT F T\nT\nT\nT\nF\nT T\n')

    def test_scratch_preconnected_and_unconnected(self):
        self.check('''implicit none
integer::u,ios
logical::exists,opened,named
character(9)::action
inquire(unit=20,exist=exists,opened=opened)
print *,exists,opened
open(newunit=u,status='scratch')
inquire(unit=u,exist=exists,opened=opened,named=named,action=action)
print *,exists,opened,named,action=='READWRITE'
close(u)
inquire(unit=u,exist=exists,opened=opened)
print *,exists,opened
inquire(unit=5,exist=exists,opened=opened,action=action,iostat=ios)
print *,exists,opened,trim(action)=='READ',ios
inquire(unit=6,exist=exists,opened=opened,action=action)
print *,exists,opened,trim(action)=='WRITE'
inquire(unit=0,exist=exists,opened=opened,action=action)
print *,exists,opened,trim(action)=='WRITE'
end
''','T F\nT T F T\nF F\nT T T 0\nT T T\nT T T\n')

    def test_iomsg_and_err(self):
        self.check('''implicit none
integer::ios,u
character(80)::message
logical::exists
message='unchanged'
inquire(unit=20,exist=exists,iostat=ios,iomsg=message,err=100)
print *,ios,trim(message)=='unchanged'
u=-1
inquire(unit=u,iostat=ios,iomsg=message,err=100)
error stop 'missed ERR transfer'
100 print *,ios/=0,len_trim(message)>0
message='unchanged'
inquire(unit=u,iostat=ios,iomsg=message)
print *,ios/=0,len_trim(message)>0
inquire(unit=u,iomsg=message,err=200)
error stop 'missed ERR without IOSTAT'
200 print *,len_trim(message)>0
end
''','0 T\nT T\nT T\nT\n')

    def test_err_in_procedure_and_loop(self):
        self.check('''program main
implicit none
integer::ios,k
logical::handled
character(80)::message
 do k=1,2
    call query(-1,ios,message,handled)
    print *,handled,ios/=0,len_trim(message)>0
 end do
contains
subroutine query(u,ios,message,handled)
integer,intent(in)::u
integer,intent(out)::ios
character(*),intent(out)::message
logical,intent(out)::handled
inquire(unit=u,iostat=ios,iomsg=message,err=100)
handled=.false.
return
100 handled=.true.
end subroutine
end program
''','T T T\nT T T\n')

    def test_backward_err(self):
        self.check('''implicit none
integer::u,attempts,ios
attempts=0
u=-1
100 attempts=attempts+1
if (attempts==2) u=20
inquire(unit=u,iostat=ios,err=100)
print *,attempts,ios
end
''','2 0\n')

    def test_iomsg_does_not_handle_error(self):
        self.check("character(50)::message\ninquire(unit=-1,iomsg=message)\nend\n",
                   error='INQUIRE failed',reference=False)

    def test_empty_filename_and_short_iomsg(self):
        self.check('''logical::exists
integer::ios,u
character(7)::message
inquire(file='',exist=exists,iostat=ios)
print *,exists,ios
u=-1
message='???????'
inquire(unit=u,iostat=ios,iomsg=message)
print *,ios/=0,len_trim(message)>0,message/='???????'
end
''','F 0\nT T T\n')

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
            ('unit=20,exist=n','LOGICAL'),
            ('unit=20,named=n','LOGICAL'),
            ('unit=20,action=n','CHARACTER'),
            ('unit=20,iomsg=n','CHARACTER'),
            ('unit=20,exist=opened,exist=opened','duplicate exist'),
            ('unit=20,err=999','undefined statement label'),
        ]:
            with self.subTest(controls=controls):
                self.reject('integer::n\nlogical::opened\ninquire('+controls+')\nend\n',message)
        self.reject('logical::ios\ninquire(unit=20,iostat=ios,err=100)\n100 continue\nend\n','INTEGER')
        self.reject('integer::ios\ninquire(unit=20,iostat=ios,iostat=ios,err=100)\n100 continue\nend\n','duplicate iostat')
        self.reject('pure subroutine f()\nlogical::opened\ninquire(unit=20,opened=opened)\nend subroutine\n',
                    'PURE procedure cannot perform external I/O')


if __name__=='__main__':unittest.main()
