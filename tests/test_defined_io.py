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

    def test_reject_unsupported_parent_forms(self):
        self.reject(self.source('iostat=0',"write(10,'(dt)') a"),'requires standard output')
        self.reject(self.source('iostat=0',"write(*,'(dt,dt)') a,b"),'one scalar object')



if __name__ == '__main__':
    unittest.main()
