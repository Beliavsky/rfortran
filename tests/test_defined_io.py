"""Defined formatted output, interpreter/native O0/O2 and gfortran."""
import unittest
import test_deallocate
from test_record_read import ROOT


class DefinedIO(unittest.TestCase):
    check = test_deallocate.Deallocate.check
    reject = test_deallocate.Deallocate.reject

    def test_scalar_parent_and_child_records(self):
        self.check((ROOT/'tests/programs/xdefined_formatted_output.f90').read_text(),
                   ' begin [7]! [9]! end\n[7]!\n done\n')

    def source(self, child, main="write(*,'(dt)') a"):
        source=(ROOT/'tests/programs/xdefined_formatted_output.f90').read_text()
        start=source.index('write(unit,')
        end=source.index('end subroutine',start)
        source=source[:start]+child+'\n'+source[end:]
        start=source.index("print *, 'begin'")
        return source[:start]+main+'\nend program\n'

    def test_dt_string_and_parameters(self):
        self.check(self.source("""write(unit,'(a,1x,i0)') iotype,size(v_list)
if(size(v_list)>0) write(unit,'(" ",i0," ",i0)') v_list
iostat=0""", """write(*,"(dt'label'(3,2))") a
print *, a"""), 'DTlabel 2 3 2\n LISTDIRECTED 0\n')

    def test_list_directed_child_and_early_return(self):
        self.check(self.source("""write(unit,*) dtv%x
iostat=0
return""", "print *, a,b\nprint *, 'after'"), '7 9\n after\n')

    def test_child_helper_subroutine(self):
        source=self.source("call emit(unit,dtv%x)\niostat=0", "print *, a,b\nprint *, 'after'")
        source=source.replace('end module', """subroutine emit(unit,value)
integer,intent(in) :: unit,value
write(unit,'("<",i0,">")') value
end subroutine
end module""")
        self.check(source,' <7> <9>\n after\n')

    def test_invalid_signature_arity(self):
        self.reject(self.source('iostat=0').replace('unit,iotype,v_list,iostat,iomsg)',
                                                   'unit,iotype,v_list,iostat)'),
                    'requires six dummy arguments')

    def test_defined_procedure_error(self):
        self.check(self.source("iostat=17\niomsg='defined output failure'"),
                   error='defined output failure',reference=False)

    def test_parent_status_and_message(self):
        source=self.source("iostat=17\niomsg='defined output failure'", """write(*,'(dt)',iostat=status,iomsg=message) a
print *,status,trim(message)
print *, 'after'""")
        source=source.replace('type(point) :: a,b','type(point) :: a,b\ninteger :: status\ncharacter(80) :: message')
        self.check(source,'17 defined output failure\n after\n')

    def test_positioning_limit_is_diagnosed(self):
        self.check(self.source("write(unit,'(t5,i0)') dtv%x\niostat=0"),
                   error='T/TL positioning in defined child output',reference=False)

    def test_reject_invalid_interface(self):
        source=self.source('iostat=0').replace('integer,intent(out) :: iostat',
                                             'integer,intent(inout) :: iostat')
        self.reject(source,'requires intent(out)')
        self.reject(self.source('iostat=0').replace('character(len=*),intent(in) :: iotype',
                                                   'character(len=20),intent(in) :: iotype'),
                    'must be CHARACTER(len=*) scalar')

    def test_reject_array_items_and_private_bindings(self):
        source=self.source('iostat=0', 'print *, a(:)')
        source=source.replace('type(point) :: a,b','type(point) :: a(2),b').replace('a%x=7','a%x=7')
        self.reject(source,'requires scalar derived variables')
        self.reject(self.source('iostat=0','print *, [a,b]'),'requires scalar derived variables')
        self.reject(self.source('iostat=0').replace('generic :: write', 'generic,private :: write'),
                    'PUBLIC accessibility only')

    def file_source(self, child, main):
        return self.source(child, main).replace('type(point) :: a,b',
            'type(point) :: a,b\ninteger :: u,status\ncharacter(100) :: line,message')

    def test_scratch_file_child_records(self):
        self.check((ROOT/'tests/programs/xdefined_formatted_file.f90').read_text(),
                   '0 unchanged\n begin [7]! [9]! end\n[7]!\n')

    def test_child_format_error_returns_status(self):
        source=self.file_source('''character(8) :: fmt
fmt='(f5.1)'
write(unit,fmt,iostat=iostat,iomsg=iomsg) dtv%x''', '''message='unchanged'
write(*,"(dt'(a)')",iostat=status,iomsg=message) a
print *,status/=0,len_trim(message)>0
print *, 'after'
''')
        self.check(source,'T T\n after\n')

    def test_other_unit_keeps_records(self):
        self.check(self.file_source('''write(*,'(a)') 'other'
write(unit,'(i0)',iostat=iostat,iomsg=iomsg) dtv%x''', '''open(newunit=u,status='scratch')
write(u,*) a,b
rewind(u)
read(u,'(a)') line
print '(a)',trim(line)
close(u)'''), 'other\nother\n 7 9\n')

    def test_invalid_parent_unit_returns_status(self):
        self.check(self.file_source('iostat=0', '''u=-99
message='unchanged'
write(u,'(dt)',iostat=status,iomsg=message) a
print *,status/=0,len_trim(message)>0
print *, 'after'
'''), 'T T\n after\n')

    def test_dynamic_stdout_and_positive_file(self):
        self.check(self.file_source('''write(unit,'(i0)',iostat=iostat,iomsg=iomsg) dtv%x''', '''u=6
write(u,'(dt)') a
open(unit=21,status='scratch')
write(21,'(dt)') b
rewind(21)
read(21,'(a)') line
print '(a)',trim(line)
close(21)'''), '7\n9\n')

    def test_readonly_parent_returns_status(self):
        self.check(self.file_source("iostat=0", """open(newunit=u,file='readonly.txt',status='replace')
write(u,'(a)') 'original'
close(u)
open(newunit=u,file='readonly.txt',status='old',action='read')
write(u,'(dt)',iostat=status,iomsg=message) a
print *,status/=0,len_trim(message)>0
read(u,'(a)') line
print '(a)',trim(line)
close(u)"""), 'T T\noriginal\n')

    def test_malformed_child_format_and_context_restoration(self):
        self.check(self.file_source("""character(8) :: fmt
fmt='invalid'
write(unit,fmt,iostat=iostat,iomsg=iomsg) dtv%x""", """open(newunit=u,status='scratch')
write(u,'(dt)',iostat=status,iomsg=message) a
print *,status/=0,len_trim(message)>0
close(u)
open(newunit=u,status='scratch')
write(u,'(a)') 'after'
rewind(u)
read(u,'(a)') line
print '(a)',trim(line)
close(u)"""), 'T T\nafter\n')

    def test_child_iomsg_unchanged_on_success(self):
        self.check(self.file_source("""iomsg='unchanged'
write(unit,'(i0)',iostat=iostat,iomsg=iomsg) dtv%x
if (iostat==0 .and. iomsg/='unchanged') error stop 'IOMSG changed'
""", """write(*,'(dt)') a
print *, 'after'
"""), '7\n after\n')

    def test_nested_defined_output(self):
        source=self.file_source("""type(leaf) :: inner
inner%x=dtv%x
write(unit,'(dt)',iostat=iostat,iomsg=iomsg) inner
if(iostat/=0) return
write(unit,'(a)',iostat=iostat,iomsg=iomsg) '!'
""", """open(newunit=u,status='scratch')
write(u,*) a,b
rewind(u)
read(u,'(a)') line
print '(a)',trim(line)
close(u)
print *, 'after'
""")
        source=source.replace('end type', """end type
type :: leaf
 integer :: x
contains
 procedure :: write_leaf
 generic :: write(formatted) => write_leaf
end type""",1)
        source=source.replace('end module', """subroutine write_leaf(dtv,unit,iotype,v_list,iostat,iomsg)
class(leaf),intent(in) :: dtv
integer,intent(in) :: unit
character(len=*),intent(in) :: iotype
integer,intent(in) :: v_list(:)
integer,intent(out) :: iostat
character(len=*),intent(inout) :: iomsg
write(unit,'("[",i0,"]")',iostat=iostat,iomsg=iomsg) dtv%x
end subroutine
end module""")
        self.check(source,' [7]! [9]!\n after\n')

    def test_reject_unsupported_parent_forms(self):
        self.reject(self.source('iostat=0',"write(*,'(dt,dt)') a,b"),'one scalar object')



if __name__ == '__main__':
    unittest.main()
