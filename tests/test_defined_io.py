"""Defined formatted output, interpreter/native O0/O2 and gfortran."""
import unittest
import test_deallocate
from test_record_read import ROOT


class DefinedIO(unittest.TestCase):
    check = test_deallocate.Deallocate.check
    reject = test_deallocate.Deallocate.reject

    def test_inherited_and_overridden_writers(self):
        source=(ROOT/'tests/programs/xdefined_formatted_inherited.f90').read_text()
        self.check(source,'base1 child2:20 base3 child4:40\nbase1 child2:20 base3 child4:40\n')

    def test_inherited_writer_array(self):
        source=(ROOT/'tests/programs/xdefined_formatted_inherited.f90').read_text()
        source=source.replace('type(grandchild) :: d','type(grandchild) :: d,values(2)')
        source=source.replace('print *,a,b,c,d',"values(1)=d\nvalues(2)=d\nwrite(*,'(*(dt,:,1x))') values")
        self.check(source,'child4:40 child4:40\nbase1 child2:20 base3 child4:40\n')

    def test_inherited_writer_wrong_override_dummy(self):
        source=(ROOT/'tests/programs/xdefined_formatted_inherited.f90').read_text()
        source=source.replace('procedure :: emit => write_child','procedure :: emit => write_base')
        self.reject(source,'defined formatted output DTV must be a scalar CLASS(')

    def test_inherited_writer_through_class_dummy(self):
        source=(ROOT/'tests/programs/xdefined_formatted_inherited.f90').read_text()
        source=source.replace('print *,a,b,c,d','call show(b)')
        source=source.replace('end module', """subroutine show(value)
class(base),intent(in) :: value
write(*,'(dt)') value
end subroutine
end module""")
        self.check(source,'child2:20\nbase1 child2:20 base3 child4:40\n')

    def test_inherited_writer_use_only(self):
        source=(ROOT/'tests/programs/xdefined_formatted_inherited.f90').read_text()
        source=source.replace('use inherited_output','use inherited_output, only: base, child, alias_child, grandchild')
        self.check(source,'base1 child2:20 base3 child4:40\nbase1 child2:20 base3 child4:40\n')

    def test_inherited_writer_private_implementation(self):
        source=(ROOT/'tests/programs/xdefined_formatted_inherited.f90').read_text()
        source=source.replace('implicit none','implicit none\nprivate\npublic :: base, child, alias_child, grandchild',1)
        self.check(source,'base1 child2:20 base3 child4:40\nbase1 child2:20 base3 child4:40\n')

    def test_inherited_writer_allocatable_dynamic_object(self):
        source=(ROOT/'tests/programs/xdefined_formatted_inherited.f90').read_text()
        source=source.replace('type(base) :: a','type(base) :: a\nclass(base),allocatable :: poly')
        source=source.replace('print *,a,b,c,d',"""allocate(child::poly)
select type(poly)
type is(child)
 poly%x=5
 poly%y=50
end select
print *,poly
""")
        self.check(source,'child5:50\nbase1 child2:20 base3 child4:40\n')

    def test_inherited_writer_across_modules(self):
        source=(ROOT/'tests/programs/xdefined_formatted_inherited.f90').read_text()
        start=source.index('type, extends(base)')
        definitions_end=source.index('contains\nsubroutine write_base')
        base_start=source.index('subroutine write_base')
        base_end=source.index('end subroutine',base_start)+len('end subroutine')
        module_end=source.index('end module')
        source=(source[:start]+'contains\n'+source[base_start:base_end]+'\nend module\n'
                +'module children\nuse inherited_output, only: base\nimplicit none\n'
                +source[start:definitions_end]+'contains\n'+source[base_end:module_end]
                +'end module\n'+source[module_end+len('end module'):].replace('use inherited_output','use children'))
        self.check(source,'base1 child2:20 base3 child4:40\nbase1 child2:20 base3 child4:40\n')

    def test_defined_writer_reference_does_not_expose_implementation(self):
        source=(ROOT/'tests/programs/xdefined_formatted_inherited.f90').read_text()
        source=source.replace('use inherited_output','use inherited_output, only: base, child, alias_child, grandchild')
        source=source.replace('print *,a,b,c,d','print *,a,b,c,d\ncall write_base()')
        self.reject(source,'is not accessible in this scope')

    def test_explicit_parent_output(self):
        source=(ROOT/'tests/programs/xdefined_formatted_inherited.f90').read_text()
        source=source.replace('print *,a,b,c,d','print *,b%base,d%child,d%child%base')
        self.check(source,'base2 child4:40 base4\nbase1 child2:20 base3 child4:40\n')

    def test_explicit_parent_array_output(self):
        source=(ROOT/'tests/programs/xdefined_formatted_inherited.f90').read_text()
        source=source.replace('type(grandchild) :: d','type(grandchild) :: d,values(2)')
        source=source.replace('print *,a,b,c,d',
            "values(1)=d\nvalues(2)=d\nprint *,values%base\nwrite(*,'(*(dt,:,1x))') values%child")
        self.check(source,'base4 base4\nchild4:40 child4:40\nbase1 child2:20 base3 child4:40\n')

    def test_child_writer_delegates_to_parent(self):
        source=(ROOT/'tests/programs/xdefined_formatted_inherited.f90').read_text()
        start=source.index('write(unit,',source.index('subroutine write_child'))
        end=source.index('\n',start)
        source=source[:start]+"write(unit,'(dt)',iostat=iostat,iomsg=iomsg) dtv%base"+source[end:]
        self.check(source,'base1 base2 base3 base4\nbase1 base2 base3 base4\n')

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
        self.reject(self.source('iostat=0','print *, [a,b]'),'requires a whole array or triplet section')
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

    def test_scalar_components_and_constructors(self):
        self.check((ROOT/'tests/programs/xdefined_formatted_values.f90').read_text(),
                   '11 12 13\n11\n')

    def test_array_elements(self):
        source=self.source("write(unit,'(i0)',iostat=iostat,iomsg=iomsg) dtv%x", """points(1)=point(21)
points(2)=point(22)
print *, points(2),points(1)
write(*,'(dt)') points(2)
""").replace('type(point) :: a,b','type(point) :: a,b,points(2)')
        self.check(source,'22 21\n22\n')

    def test_function_result_evaluated_once(self):
        source=self.source("write(unit,'(i0)',iostat=iostat,iomsg=iomsg) dtv%x", """print *, make_point()
print *, calls
write(*,'(dt)') make_point()
print *, calls
""")
        source=source.replace('implicit none', 'implicit none\ninteger :: calls=0',1)
        source=source.replace('end module', """function make_point() result(p)
type(point) :: p
calls=calls+1
p%x=30+calls
end function
end module""")
        self.check(source,'31\n1\n32\n2\n')

    def test_components_of_array_elements(self):
        source=self.source("write(unit,'(i0)',iostat=iostat,iomsg=iomsg) dtv%x", """boxes(1)%item=point(41)
boxes(2)%item=point(42)
print *, boxes(2)%item,boxes(1)%item
""")
        source=source.replace('end type', 'end type\ntype :: container\n type(point) :: item\nend type',1)
        source=source.replace('type(point) :: a,b','type(point) :: a,b\ntype(container) :: boxes(2)')
        self.check(source,'42 41\n')

    def test_scalar_elements_of_array_components(self):
        source=self.source("write(unit,'(i0)',iostat=iostat,iomsg=iomsg) dtv%x", """box%items(1)=point(51)
box%items(2)=point(52)
print *, box%items(2),box%items(1)
""")
        source=source.replace('end type', 'end type\ntype :: container\n type(point) :: items(2)\nend type',1)
        source=source.replace('type(point) :: a,b','type(point) :: a,b\ntype(container) :: box')
        self.check(source,'52 51\n')

    def test_vector_subscripts(self):
        source=self.source("write(unit,'(i0)',iostat=iostat,iomsg=iomsg) dtv%x", """points(1)=point(10)
points(2)=point(20)
points(3)=point(30)
indices=[3,1,3]
print *, points(indices)
print *, points([2,1,2])
print *, 1,points([integer ::]),2
""").replace('type(point) :: a,b',
            'type(point) :: a,b,points(3)\ninteger :: indices(-1:1)')
        self.check(source,'30 10 30\n20 10 20\n1 2\n')

    def test_mixed_vector_sections(self):
        self.check((ROOT/'tests/programs/xdefined_formatted_vectors.f90').read_text(),
                   '22 20 22 12 10 12\n21 20 11 10 21 20\n')

    def test_vector_expression_evaluated_once(self):
        source=self.source("write(unit,'(i0)',iostat=iostat,iomsg=iomsg) dtv%x", """points(1)=point(10)
points(2)=point(20)
print *,points(indices())
print *,calls
""").replace('type(point) :: a,b','type(point) :: a,b,points(2)')
        source=source.replace('implicit none','implicit none\ninteger :: calls=0',1)
        source=source.replace('end module', """function indices() result(v)
integer :: v(3)
calls=calls+1
v=[2,1,2]
end function
end module""")
        self.check(source,'20 10 20\n1\n')

    def test_runtime_vector_dummy(self):
        source=self.source("write(unit,'(i0)',iostat=iostat,iomsg=iomsg) dtv%x", """points(1)=point(10)
points(2)=point(20)
call emit(points,[2,1,2])
call emit(points,[integer ::])
""").replace('type(point) :: a,b','type(point) :: a,b,points(2)')
        source=source.replace('end module', """subroutine emit(values,indices)
type(point),intent(in) :: values(:)
integer,intent(in) :: indices(:)
print *,1,values(indices),2
end subroutine
end module""")
        self.check(source,'1 20 10 20 2\n1 2\n')

    def test_vector_wide_integer_kind(self):
        source=self.source("write(unit,'(i0)',iostat=iostat,iomsg=iomsg) dtv%x", """points(1)=point(10)
points(2)=point(20)
indices=[2,1]
print *,points(indices)
""").replace('type(point) :: a,b',
            'type(point) :: a,b,points(2)\ninteger(kind=selected_int_kind(18)) :: indices(2)')
        self.check(source,'20 10\n')

    def test_vector_error_stops_iteration(self):
        source=(ROOT/'tests/programs/xdefined_formatted_array_error.f90').read_text()
        source=source.replace(') points,b',') points([3,2,1,2]),b')
        self.check(source,'17 element failed 2\n',reference=False)

    def test_subscript_evaluated_once(self):
        source=self.source("write(unit,'(i0,1x,i0)',iostat=iostat,iomsg=iomsg) dtv%x,dtv%y", """points(1)=point(61,62)
points(2)=point(71,72)
print *, points(next_index())
print *, calls
""")
        source=source.replace('integer :: x','integer :: x,y',1)
        source=source.replace('implicit none','implicit none\ninteger :: calls=0',1)
        source=source.replace('type(point) :: a,b','type(point) :: a,b,points(2)')
        source=source.replace('end module', """integer function next_index() result(i)
calls=calls+1
i=calls
end function
end module""")
        self.check(source,'61 62\n1\n')

    def test_multidimensional_element_and_array_result_diagnostic(self):
        source=self.source("write(unit,'(i0)',iostat=iostat,iomsg=iomsg) dtv%x", """points(2,1)=point(81)
print *, points(2,1)
""").replace('type(point) :: a,b','type(point) :: a,b,points(2,2)')
        self.check(source,'81\n')
        source=self.source('iostat=0','print *, make_points()').replace('end module', """function make_points() result(p)
type(point) :: p(2)
p(1)=point(1)
p(2)=point(2)
end function
end module""")
        self.reject(source,'requires a whole array or triplet section')

    def test_whole_array_list_output(self):
        source=self.source("write(unit,'(i0)',iostat=iostat,iomsg=iomsg) dtv%x", """points(-1)=point(10)
points(0)=point(20)
points(1)=point(30)
print *, 'values',points,'end'
""").replace('type(point) :: a,b','type(point) :: a,b,points(-1:1)')
        self.check(source,' values 10 20 30 end\n')

    def test_multidimensional_array_element_order(self):
        self.check((ROOT/'tests/programs/xdefined_formatted_arrays.f90').read_text(),
                   '1 2 3 4\n')

    def test_allocatable_array_and_empty_array(self):
        source=self.source("write(unit,'(i0)',iostat=iostat,iomsg=iomsg) dtv%x", """n=3
allocate(points(n))
points(1)=point(7)
points(2)=point(8)
points(3)=point(9)
print *, points
print *, 1,empty,2
deallocate(points)
""").replace('type(point) :: a,b','type(point) :: a,b,empty(0)\ntype(point),allocatable :: points(:)\ninteger :: n')
        self.check(source,'7 8 9\n1 2\n')

    def test_array_stops_after_writer_error(self):
        # Positive defined-I/O IOSTAT signals an error (NAG Fortran manual 9.7).
        # Local gfortran 17 continues the array and overwrites status with zero.
        # https://support.nag.com/nagware/np/r72_doc/manual/compiler_9_7.html
        self.check((ROOT/'tests/programs/xdefined_formatted_array_error.f90').read_text(),
                   '17 element failed 2\n',reference=False)

    def test_rank_three_allocatable_and_assumed_shape(self):
        source=self.source("write(unit,'(i0)',iostat=iostat,iomsg=iomsg) dtv%x", """allocate(points(2,1,2))
points(1,1,1)=point(1)
points(2,1,1)=point(2)
points(1,1,2)=point(3)
points(2,1,2)=point(4)
call emit(points)
deallocate(points)
""").replace('type(point) :: a,b','type(point) :: a,b\ntype(point),allocatable :: points(:,:,:)')
        source=source.replace('end module', """subroutine emit(values)
type(point),intent(in) :: values(:,:,:)
print *,values
end subroutine
end module""")
        self.check(source,'1 2 3 4\n')

    def test_rank_fifteen_single_element(self):
        dimensions=','.join(['1']*15)
        source=self.source("write(unit,'(i0)',iostat=iostat,iomsg=iomsg) dtv%x",
                           f'points({dimensions})=point(15)\nprint *, points')
        source=source.replace('type(point) :: a,b',f'type(point) :: a,b,points({dimensions})')
        self.check(source,'15\n')

    def test_whole_array_component_to_file(self):
        source=self.file_source("write(unit,'(i0)',iostat=iostat,iomsg=iomsg) dtv%x", """box%points(1)=point(91)
box%points(2)=point(92)
open(newunit=u,status='scratch')
write(u,*) box%points
rewind(u)
read(u,'(a)') line
print '(a)',trim(line)
close(u)
""")
        source=source.replace('end type','end type\ntype :: container\n type(point) :: points(2)\nend type',1)
        source=source.replace('type(point) :: a,b','type(point) :: a,b\ntype(container) :: box')
        self.check(source,'91 92\n')

    def test_multidimensional_error_stops_outer_iterations(self):
        source=self.file_source("""calls=calls+1
iostat=21
iomsg='first element failed'
""", """points%x=1
open(newunit=u,status='scratch')
write(u,*,iostat=status,iomsg=message) points
print *,status,trim(message),calls
close(u)
""").replace('type(point) :: a,b','type(point) :: a,b,points(2,2,2)')
        source=source.replace('implicit none','implicit none\ninteger :: calls=0',1)
        self.check(source,'21 first element failed 1\n',reference=False)

    def test_formatted_array_example(self):
        self.check((ROOT/'tests/programs/xdefined_formatted_array_formats.f90').read_text(),
                   '22, 20, 22, 12, 10, 12\n21 20\n11 10\n21 20\n')

    def test_partial_repeated_dt_descriptor(self):
        source=self.source("write(unit,'(i0)',iostat=iostat,iomsg=iomsg) dtv%x", """points(1)=point(7)
write(*,'(2dt,"unreached")') points
write(*,'(2dt,"unreached")') a
""").replace('type(point) :: a,b','type(point) :: a,b,points(1)')
        self.check(source,'7\n7\n')

    def test_formatted_array_reversion(self):
        source=self.source("write(unit,'(i0)',iostat=iostat,iomsg=iomsg) dtv%x", """points(1)=point(1)
points(2)=point(2)
points(3)=point(3)
write(*,'(2(dt,1x))') points,b
""").replace('type(point) :: a,b','type(point) :: a,b,points(3)')
        self.check(source,'1 2\n3 9\n')

    def test_formatted_mixed_arrays(self):
        source=self.source("write(unit,'(i0)',iostat=iostat,iomsg=iomsg) dtv%x", """points(1)=point(7)
points(2)=point(9)
values=[2,3]
write(*,'(a,2(i0,1x),2(dt,1x),a)') 'start ',values,points,'end'
""").replace('type(point) :: a,b','type(point) :: a,b,points(2)\ninteger :: values(2)')
        self.check(source,'start 2 3 7 9 end\n')

    def test_formatted_runtime_array_sections(self):
        source=self.source("write(unit,'(i0)',iostat=iostat,iomsg=iomsg) dtv%x", """n=4
allocate(points(0:n))
do i=0,n
 points(i)=point(i)
end do
write(*,'(*(dt,:,", "))') points(n:0:-2),points([1,3])
write(*,'(*(dt,:,", "))') points(1:0)
deallocate(points)
""").replace('type(point) :: a,b','type(point) :: a,b\ntype(point),allocatable :: points(:)\ninteger :: i,n')
        self.check(source,'4, 2, 0, 1, 3\n\n')

    def test_formatted_array_different_vlists(self):
        source=self.source("write(unit,'(a,i0,i0)',iostat=iostat,iomsg=iomsg) iotype,v_list(1),dtv%x", """points(1)=point(7)
points(2)=point(9)
write(*,'(dt"x"(1),1x,dt"y"(2))') points
""").replace('type(point) :: a,b','type(point) :: a,b,points(2)')
        self.check(source,'DTx17 DTy29\n')

    def test_formatted_array_error_stops_iteration(self):
        source=(ROOT/'tests/programs/xdefined_formatted_array_error.f90').read_text()
        source=source.replace('write(u,*,iostat=',"write(u,'(*(dt,1x))',iostat=")
        self.check(source,'17 element failed 2\n',reference=False)

    def test_formatted_empty_array_controls(self):
        source=self.source("write(unit,'(i0)',iostat=iostat,iomsg=iomsg) dtv%x", """write(*,'("outer",*("inner",dt))') points
write(*,'("before",dt,"after")') points
write(*,'(dt,"between",:,"after")') a,points
""").replace('type(point) :: a,b','type(point) :: a,b,points(0)')
        self.check(source,'outerinner\nbefore\n7between\n')

    def test_formatted_assumed_shape_matrix(self):
        source=self.source("write(unit,'(i0)',iostat=iostat,iomsg=iomsg) dtv%x", """do j=1,2
 do i=0,2
  points(i,j)=point(10*j+i)
 end do
end do
call emit(points)
""").replace('type(point) :: a,b','type(point) :: a,b,points(0:2,2)\ninteger :: i,j')
        source=source.replace('end module', """subroutine emit(values)
type(point),intent(in) :: values(:,:)
write(*,'(*(dt,:,", "))') values(3:1:-1,[2,1])
end subroutine
end module""")
        self.check(source,'22, 21, 20, 12, 11, 10\n')

    def test_formatted_array_large_runtime_extent(self):
        source=self.source("calls=calls+1\niostat=0", """n=2000
allocate(points(n))
points%x=1
write(*,'(*(dt))') points
print *,calls
""").replace('type(point) :: a,b','type(point) :: a,b\ntype(point),allocatable :: points(:)\ninteger :: n')
        source=source.replace('implicit none','implicit none\ninteger :: calls=0',1)
        self.check(source,'2000\n')

    def test_formatted_array_mismatch_status(self):
        source=self.file_source('iostat=0', """points(1)=point(1)
open(newunit=u,status='scratch')
write(u,'(i0)',iostat=status,iomsg=message) points
print *,status>0,len_trim(message)>0
close(u)
""").replace('type(point) :: a,b','type(point) :: a,b,points(1)')
        self.check(source,'T T\n',reference=False)

    def test_triplet_sections(self):
        source=self.source("write(unit,'(i0)',iostat=iostat,iomsg=iomsg) dtv%x", """do i=0,5
 points(i)=point(i)
end do
step=-2
print *, points(::2)
print *, points(5:1:step)
print *, 10,points(5:1),20
print *, 10,points(::-1),20
""").replace('type(point) :: a,b','type(point) :: a,b,points(0:5)\ninteger :: i,step')
        self.check(source,'0 2 4\n5 3 1\n10 20\n10 20\n')

    def test_multidimensional_triplet_sections(self):
        self.check((ROOT/'tests/programs/xdefined_formatted_sections.f90').read_text(),
                   '12 11 10 22 21 20\n21 11\n')

    def test_section_controls_evaluated_once(self):
        source=self.source("write(unit,'(i0)',iostat=iostat,iomsg=iomsg) dtv%x", """points(1,1)=point(1)
points(2,1)=point(2)
points(3,1)=point(3)
print *, points(bound(1):bound(3):bound(2),bound(1))
print *, calls
""").replace('type(point) :: a,b','type(point) :: a,b,points(3,1)')
        source=source.replace('implicit none','implicit none\ninteger :: calls=0',1)
        source=source.replace('end module', """integer function bound(i) result(j)
integer,intent(in) :: i
calls=calls+1
j=i
end function
end module""")
        self.check(source,'1 3\n4\n')

    def test_zero_section_stride(self):
        source=self.source('iostat=0',"step=0\nprint *, points(1:2:step)").replace(
            'type(point) :: a,b','type(point) :: a,b,points(2)\ninteger :: step')
        self.check(source,error='array section stride must not be zero',reference=False)

    def test_runtime_allocatable_section_to_file(self):
        source=self.file_source("write(unit,'(i0)',iostat=iostat,iomsg=iomsg) dtv%x", """n=5
allocate(points(-1:n))
do i=-1,n
 points(i)=point(i)
end do
open(newunit=u,status='scratch')
call emit(u,points)
rewind(u)
read(u,'(a)') line
print '(a)',trim(line)
close(u)
deallocate(points)
""").replace('type(point) :: a,b','type(point) :: a,b\ntype(point),allocatable :: points(:)\ninteger :: n,i')
        source=source.replace('end module', """subroutine emit(unit,values)
integer,intent(in) :: unit
type(point),intent(in) :: values(:)
write(unit,*) values(size(values):1:-2)
end subroutine
end module""")
        self.check(source,'5 3 1 -1\n')

    def test_section_error_stops_iteration(self):
        source=(ROOT/'tests/programs/xdefined_formatted_array_error.f90').read_text()
        source=source.replace(') points,b',') points(1:3),b')
        self.check(source,'17 element failed 2\n',reference=False)

    def test_reject_noninteger_section_bound(self):
        source=self.source('iostat=0','print *, points(first:2)').replace(
            'type(point) :: a,b','type(point) :: a,b,points(2)\nreal :: first=1.0')
        self.reject(source,'section bounds and stride must be scalar INTEGER')

    def test_array_component_section(self):
        source=self.source("write(unit,'(i0)',iostat=iostat,iomsg=iomsg) dtv%x", """box%points(1)=point(1)
box%points(2)=point(2)
print *,box%points(2:1:-1)
""")
        source=source.replace('end type','end type\ntype :: container\n type(point) :: points(2)\nend type',1)
        source=source.replace('type(point) :: a,b','type(point) :: a,b\ntype(container) :: box')
        self.check(source,'2 1\n')

    def test_mixed_parent_format(self):
        self.check(self.source("write(unit,'(i0)',iostat=iostat,iomsg=iomsg) dtv%x",
            "write(*,'(a,dt,a,i3,1x,dt,a)') 'left=',a,', n=',12,b,'!'"),
            'left=7, n= 12 9!\n')

    def test_repeated_parent_dt(self):
        self.check(self.source("write(unit,'(i0)',iostat=iostat,iomsg=iomsg) dtv%x",
            "write(*,'(2(dt,1x))') a,b"), '7 9\n')

    def test_parent_format_reversion(self):
        self.check(self.source("write(unit,'(i0)',iostat=iostat,iomsg=iomsg) dtv%x",
            "write(*,'(dt)') a,b"), '7\n9\n')

    def test_parent_dt_parameters_and_colon(self):
        self.check(self.source("write(unit,'(a,i0,i0)',iostat=iostat,iomsg=iomsg) iotype,v_list(1),dtv%x",
            """write(*,'(dt"x"(1),1x,dt"y"(2),:,"unused")') a,b"""), 'DTx17 DTy29\n')

    def test_parent_modes_persist(self):
        self.check(self.source("write(unit,'(i0)',iostat=iostat,iomsg=iomsg) dtv%x",
            "write(*,'(sp,i0,1x,dt,1x,i0,ss,1x,i0)') 2,a,3,4"), '+2 7 +3 4\n')

    def test_unlimited_parent_group(self):
        self.check(self.source("write(unit,'(i0)',iostat=iostat,iomsg=iomsg) dtv%x",
            "write(*,'(*(dt,:,1x))') a,b,a"), '7 9 7\n')

    def test_parent_reversion_to_last_group(self):
        self.check(self.source("write(unit,'(i0)',iostat=iostat,iomsg=iomsg) dtv%x",
            "write(*,'(a,(dt,1x))') 'start=',a,b,a"), 'start=7\n9\n7\n')

    def test_parent_repeated_descriptors(self):
        self.check(self.source("write(unit,'(i0)',iostat=iostat,iomsg=iomsg) dtv%x",
            "write(*,'(2i2,2dt)') 1,2,a,b"), '1 279\n')

    def test_mixed_parent_error_stops_output(self):
        source=self.file_source("""iostat=17
iomsg='writer failed'""", """open(newunit=u,status='scratch')
write(u,'(a,dt,a)',iostat=status,iomsg=message) 'before',a,'after'
print *,status,trim(message)
close(u)
""")
        self.check(source,'17 writer failed\n')

    def test_reject_parent_descriptor_mismatch(self):
        self.reject(self.source('iostat=0',"write(*,'(i0)') a"),'DT edit descriptor requires')
        self.reject(self.source('iostat=0',"write(*,'(dt,dt)') a,1"),'DT edit descriptor requires')

    def test_reject_complex_mixed_parent(self):
        self.reject(self.source('iostat=0',"write(*,'(dt,2f8.2)') a,(1.0,2.0)"),
                    'COMPLEX items mixed with defined formatted output')

    def test_reject_runtime_parent_format(self):
        source=self.source('iostat=0',"write(*,fmt) a").replace('type(point) :: a,b',
            "type(point) :: a,b\ncharacter(4) :: fmt='(dt)'")
        self.reject(source,'literal parent format')

    def test_reject_parent_positioning(self):
        self.reject(self.source('iostat=0',"write(*,'(dt,t1,a)') a,'x'"),'T/TL positioning')



if __name__ == '__main__':
    unittest.main()
