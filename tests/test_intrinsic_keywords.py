"""Intrinsic keyword association and RANDOM_SEED state round trips."""
import unittest
import test_deallocate
from test_record_read import ROOT

class IntrinsicKeywords(unittest.TestCase):
    check=test_deallocate.Deallocate.check
    reject=test_deallocate.Deallocate.reject

    def test_random_seed_reproducer(self):
        self.check((ROOT/'xrandom_seed.f90').read_text(encoding='utf-8'),'T T T\n')

    def test_seed_array_expression(self):
        self.check((ROOT/'xrandom_seed_expression.f90').read_text(encoding='utf-8'),'T\n')

    def test_seed_runtime_pack_expression(self):
        source=(ROOT/'xrandom_seed_expression.f90').read_text(encoding='utf-8')
        self.check(source.replace('put=seed+37','put=pack(seed+37,seed>0)'),'T\n')

    def test_seed_constructor_section_and_elemental_expression(self):
        self.check("""implicit none
integer::seed(8),i
real::a(3),b(3)
seed=123
call random_seed(put=[(160,i=1,8)])
call random_number(a)
call random_seed(put=abs(-seed)+37)
call random_number(b)
print *,all(a==b)
call random_seed(put=seed(8:1:-1)+37)
call random_number(b)
print *,all(a==b)
end
""",'T\nT\n')

    def test_seed_expression_types_and_output_definability(self):
        for declaration,actual in [
            ('integer::s(8)','1'),
            ('real::s(8)','s+1.0'),
            ('integer(8)::s(8)','s+1'),
            ('integer::s(2,2)','s+1'),
            ('logical::s(8)','s'),
        ]:
            with self.subTest(actual=actual,declaration=declaration):
                self.reject('implicit none\n'+declaration+'\ncall random_seed(put='+actual+')\nend\n','default INTEGER rank-one')
        self.reject('integer::s(8)\ncall random_seed(get=s+1)\nend\n','GET requires')
        self.reject('call random_seed(put=[1])\nend\n','at least 2 elements')

    def test_seed_get_saves_current_state(self):
        self.check("""implicit none
integer::n
integer,allocatable::seed(:)
real::a(5),b(5),discard
call random_seed(size=n)
allocate(seed(n))
seed=-37
call random_seed(put=seed)
call random_number(harvest=discard)
call random_seed(get=seed)
call random_number(harvest=a)
call random_seed(put=seed)
call random_number(harvest=b)
print *,all(a==b),all(a>=0.0 .and. a<1.0),n>0,discard>=0.0
end
""",'T T T T\n')

    def test_seed_size_array_element_and_positional_form(self):
        self.check("""integer::n(2)
call random_seed(n(1))
call random_seed(size=n(2))
print *,n(1)==n(2),n(1)>0
end
""",'T T\n')

    def test_initialized_seed_with_nondefault_lower_bound(self):
        self.check("""implicit none
integer::n
integer::seed(0:7)=123
real::a,b
call random_seed(size=n)
call random_seed(put=seed)
call random_seed(get=seed)
call random_number(harvest=a)
call random_seed(put=seed)
call random_number(harvest=b)
print *,a==b,n>0
end
""",'T T\n')

    def test_scalar_intrinsics(self):
        self.check("""implicit none
integer,parameter::dp=selected_real_kind(r=307,p=15)
integer,parameter::rk=selected_real_kind(r=307)
integer,parameter::ik=selected_int_kind(r=9)
real::x
x=2.75
print *,int(a=x),real(a=3),dble(a=4)
print *,sqrt(x=4.0),atan2(x=1.0,y=0.0)
print *,min(a3=2,a1=7,a2=4),max(a2=4,a3=9,a1=2),max(a1=2,a2=1,a4=3)
print *,abs(a=-3),mod(p=4,a=11),modulo(p=4,a=-1)
print *,dp,rk,ik,kind(x=1.0),digits(x=1),radix(x=x)
end
""",'2 3 4\n2 0\n2 9 3\n3 3 3\n8 8 4 4 31 2\n')

    def test_character_intrinsics(self):
        self.check("""implicit none
character(4)::s
s='a   '
print *,len(string=s),len_trim(string=s)
print *,len(string='xy',kind=8),len_trim(string='xy  ',kind=8)
print *,ichar(c='A'),iachar(c='B',kind=4)
print *,char(i=65,kind=1)//achar(i=66)
print *,trim(string=s),adjustl(string='  ab'),adjustr(string='ab  ')
end
""",'4 1\n2 2\n65 66\nAB\naab  ab\n')

    def test_array_intrinsics(self):
        self.check("""implicit none
integer::a(2,2),b(2,2)
a=reshape(shape=[2,2],source=[1,2,3,4])
b=transpose(matrix=a)
print *,sum(array=b)
print *,sum(array=pack(mask=a>2,array=a))
end
""",'10\n7\n')

    def test_seed_diagnostics(self):
        for decl,call,message in [
            ('integer::n,s(2)','call random_seed(size=n,put=s)','at most one'),
            ('integer::n','call random_seed(size=1)','SIZE requires'),
            ('real::s(2)','call random_seed(put=s)','default INTEGER'),
            ('integer(8)::s(2)','call random_seed(get=s)','default INTEGER'),
            ('integer::s(2,2)','call random_seed(get=s)','rank-one'),
            ('integer,parameter::n=2','call random_seed(size=n)','writable'),
            ('integer::s(2)','call random_seed(put=s)','uninitialized'),
            ('integer::n','call random_seed(bad=n)','unknown keyword'),
            ('integer::n','call random_seed(size=n,size=n)','duplicate argument'),
        ]:
            with self.subTest(call=call,decl=decl):self.reject('implicit none\n'+decl+'\n'+call+'\nend\n',message)

    def test_unknown_and_duplicate_intrinsic_keywords(self):
        for call,message in [('sign(a=1,b=2,b=3)','duplicate argument'),
                             ('dot_product(foo=[1,2],vector_b=[1,2])','unknown keyword'),
                             ('max(a1=2,a3=4)','missing required'),
                             ('len(string="abc",kind=3)','INTEGER KIND'),
                             ('achar(i=65,kind=4)','CHARACTER KIND')]:
            with self.subTest(call=call):self.reject('print *,'+call+'\nend\n',message)

    def test_seed_readonly_and_pure_procedure_checks(self):
        self.reject("""subroutine f(seed)
integer,intent(in)::seed(2)
if(.true.) call random_seed(get=seed)
end subroutine
end
""",'writable')
        self.reject("""pure subroutine f(n)
integer,intent(out)::n
call random_seed(size=n)
end subroutine
end
""",'PURE')

if __name__=='__main__':unittest.main(verbosity=2)
