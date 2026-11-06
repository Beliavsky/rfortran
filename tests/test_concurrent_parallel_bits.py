"""Kind-aware bit intrinsics in direct parallel loops and PURE/SIMPLE callees."""
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest
import test_concurrent_parallel as support
from test_record_read import COMPILER, ROOT


def program(kind, context, boundaries=False):
    declarations=f'integer({kind})::x,zero,ones,power,signbit\ninteger::p,bits,r,full'
    # Exponents stop below the sign bit, keeping the arithmetic reference defined.
    body=f"""x=int(modulo(i,127),{kind})
zero=0
ones=-1
bits=bit_size(x)
p=modulo(i,bits-1)
full=bits
power=int(2_8**p,{kind})
signbit=-huge(x)-1
r=0
"""
    cases=[
        ('iand(j=ones,i=x)', 'x'), ('ior(j=zero,i=x)', 'x'),
        ('ieor(j=ones,i=x)', '-x-1'), ('not(i=x)', '-x-1'),
        ('ibset(pos=p,i=zero)', 'power'), ('ibclr(pos=p,i=ones)', '-1-power'),
        ('ibits(len=1,pos=p,i=ones)', '1'), ('ishft(shift=-p,i=power)', '1'),
        ('ishft(shift=p,i=int(1,kind(x)))', 'power'),
        ('shiftl(shift=p,i=int(1,kind(x)))', 'power'),
        ('shiftr(shift=p,i=power)', '1'), ('shifta(shift=p,i=ones)', '-1'),
        ('merge_bits(mask=x,j=zero,i=ones)', 'x'),
        ('popcnt(i=power)', '1'), ('poppar(i=power)', '1'),
        ('leadz(i=power)', 'bits-p-1'), ('trailz(i=power)', 'p'),
        ('dshiftl(shift=p,j=ones,i=zero)', 'power-1'),
        ('dshiftr(shift=p,j=power,i=zero)', '1'),
    ]
    logical=['btest(pos=p,i=power)']
    if boundaries:
        cases=[
            ('ibset(zero,full-1)', 'signbit'), ('ibclr(ones,full-1)', 'huge(x)'),
            ('ibits(ones,0,full)', '-1'), ('ibits(ones,full,0)', '0'),
            ('ishft(ones,full)', '0'), ('ishft(ones,-full)', '0'),
            ('shiftl(ones,full)', '0'), ('shiftr(ones,full)', '0'),
            ('shifta(ones,full)', '-1'), ('shiftr(signbit,full-1)', '1'),
            ('dshiftl(x,ones,0)', 'x'), ('dshiftl(x,ones,full)', '-1'),
            ('dshiftr(x,ones,0)', '-1'), ('dshiftr(x,ones,full)', 'x'),
            ('popcnt(ones)', 'full'), ('poppar(ones)', '0'),
            ('leadz(zero)', 'full'), ('trailz(zero)', 'full'),
            ('leadz(signbit)', '0'), ('trailz(signbit)', 'full-1'),
            ('kind(ibset(x,p))', str(kind)), ('kind(popcnt(x))', '4'),
        ]
        logical=['btest(ones,full-1)', '.not.btest(zero,full-1)']
    if boundaries:
        body=body.replace(f'power=int(2_8**p,{kind})\n','')
        cases.append(('shifta(ones,p)','-1'))
    else:
        body=body.replace('full=bits\n','').replace('signbit=-huge(x)-1\n','')
    for expr,expected in cases:
        body+=f'if ({expr}/={expected}) r=r+1\n'
    for expr in logical:
        body+=f'if (.not.({expr})) r=r+1\n'
    if context=='direct':
        header=f'integer::i,y(9000)\n{declarations}'
        loop=body+'y(i)=r'
        locality=' local(x,zero,ones,power,signbit,p,bits,r,full)'
        tail=''
    elif context=='function':
        header='integer::i,y(9000)'
        loop='y(i)=probe(i)';locality=''
        tail=f'contains\npure integer function probe(i) result(r)\ninteger,intent(in)::i\n{declarations.replace(",r,",",")}\n{body}end function\n'
    else:
        header='integer::i,y(9000)'
        loop='call probe(i,y(i))';locality=''
        # R is explicitly declared in the common declarations.
        tail=f'contains\nsimple subroutine probe(i,r)\ninteger,intent(in)::i\ninteger,intent(out)::r\n{declarations.replace(",r,",",")}\n{body}end subroutine\n'
    return f"implicit none\n{header}\ndo concurrent(i=1:9000){locality}\n{loop}\nend do\nif(any(y/=0)) stop 1\nprint *, 'PASS'\n{tail}end\n"


class ParallelBits(unittest.TestCase):
    build_run=support.ParallelConcurrent.build_run

    def test_all_bit_intrinsics_all_kinds_and_call_contexts(self):
        for kind in (1,2,4,8):
            for context in ('direct','function','subroutine'):
                with self.subTest(kind=kind,context=context):
                    self.build_run(program(kind,context),'PASS')

    def test_sign_bits_zero_fields_and_full_width_shifts(self):
        for kind in (1,2,4,8):
            for context in ('direct','function','subroutine'):
                with self.subTest(kind=kind,context=context):
                    self.build_run(program(kind,context,True),'PASS')

    def test_integer_and_logical_function_results(self):
        for kind in (1,2,4,8):
            self.build_run(f"""implicit none
integer::i
integer({kind})::a(9000)
logical::selected(9000)
do concurrent(i=1:9000)
 a(i)=set_bit(int(0,{kind}),modulo(i,{8*kind}))
 selected(i)=test_bit(a(i),modulo(i,{8*kind}))
end do
if(.not.all(selected)) stop 1
if(a({8*kind-1})/=-huge(a(1))-1) stop 2
print *, 'PASS'
contains
pure integer({kind}) function set_bit(value,p) result(y)
integer({kind}),intent(in)::value
integer,intent(in)::p
y=ibset(value,p)
end function
simple logical function test_bit(value,p) result(tf)
integer({kind}),intent(in)::value
integer,intent(in)::p
tf=btest(value,p)
end function
end
""",'PASS')

    def test_serial_and_unchecked_results(self):
        for context in ('direct','function','subroutine'):
            source=program(8,context,True)
            self.build_run(source,'PASS',parallel=False,threads=(1,))
            self.build_run(source,'PASS',checks=False)

    @unittest.skipUnless(shutil.which('gfortran'),'gfortran unavailable')
    def test_reference_results(self):
        with tempfile.TemporaryDirectory(prefix='rfort-parallel-bits-reference-') as tmp:
            tmp=Path(tmp);path=tmp/'case.f90';exe=tmp/'case.exe'
            for kind in (1,2,4,8):
                for boundaries in (False,True):
                    path.write_text(program(kind,'function',boundaries))
                    build=subprocess.run(['gfortran','-std=f2018','-O2','-fcheck=all',str(path),'-o',str(exe)],capture_output=True,text=True,timeout=60)
                    self.assertEqual(build.returncode,0,build.stderr)
                    result=subprocess.run([str(exe)],capture_output=True,text=True,timeout=30)
                    self.assertEqual(result.returncode,0,result.stderr)
                    self.assertEqual(result.stdout.split(),['PASS'])

    def test_runtime_control_errors(self):
        for kind in (1,8):
            width=8*kind
            for expression,diagnostic in [
                ('btest(x,p)','BTEST: POS'), ('ibset(x,p)','IBSET: POS'),
                ('ibclr(x,p)','IBCLR: POS'), ('ibits(x,p,1)','IBITS: POS and LEN'),
                ('ishft(x,p+1)','ISHFT: SHIFT'), ('dshiftl(x,x,p+1)','DSHIFTL: SHIFT'),
                ('dshiftr(x,x,p+1)','DSHIFTR: SHIFT'), ('shiftl(x,p+1)','SHIFTL: SHIFT'),
                ('shiftr(x,p+1)','SHIFTR: SHIFT'), ('shifta(x,p+1)','SHIFTA: SHIFT'),
            ]:
                value=f'int({expression})'
                def assignment(target):
                    if expression.startswith('btest'):
                        return f'if ({expression}) then\n{target}=1\nelse\n{target}=0\nend if'
                    return f'{target}={value}'
                for context in ('direct','function','subroutine'):
                    loop=assignment('y(i)')
                    tail=''
                    if context!='direct':
                        loop='y(i)=probe(x,p)' if context=='function' else 'call probe(x,p,y(i))'
                        if context=='function':
                            signature='pure integer function probe(x,p) result(r)'
                            output='';end='end function'
                        else:
                            signature='simple subroutine probe(x,p,r)'
                            output='integer,intent(out)::r\n';end='end subroutine'
                        tail=f'contains\n{signature}\ninteger({kind}),intent(in)::x\ninteger,intent(in)::p\n{output}{assignment("r")}\n{end}\n'
                    source=f"implicit none\ninteger::i,p,y(9000)\ninteger({kind})::x\nx=1\np={width}\ndo concurrent(i=1:9000)\n{loop}\nend do\nprint *,y(1)\n{tail}end\n"
                    with self.subTest(kind=kind,expression=expression,context=context),tempfile.TemporaryDirectory(prefix='rfort-parallel-bits-error-') as tmp:
                        path=Path(tmp)/'case.f90';path.write_text(source);exe=Path(tmp)/'case.exe'
                        for opt in ('0','2'):
                            command=[str(COMPILER),'-O',opt,'--parallel','--parallel-report']
                            if context=='subroutine':
                                command+=['--nocheck']
                            build=subprocess.run(command+[str(path),'-o',str(exe)],capture_output=True,text=True,timeout=90)
                            self.assertEqual(build.returncode,0,build.stderr)
                            self.assertIn('native parallel worker emitted',build.stderr)
                            self.assertNotIn('serial fallback',build.stderr)
                            for threads in (1,2,4):
                                env=os.environ.copy();env['RFORT_NUM_THREADS']=str(threads)
                                result=subprocess.run([str(exe)],env=env,capture_output=True,text=True,timeout=30)
                                self.assertNotEqual(result.returncode,0)
                                self.assertIn(diagnostic,result.stderr)

    def test_constant_control_and_kind_errors(self):
        for expression,diagnostic in [
            ('ibset(0_1,8)','IBSET: POS'),
            ('ibits(0_8,63,2)','IBITS: POS and LEN'),
            ('iand(i,1_8)','same INTEGER kind'),
        ]:
            with self.subTest(expression=expression),tempfile.TemporaryDirectory(prefix='rfort-parallel-bits-constant-') as tmp:
                path=Path(tmp)/'case.f90';exe=Path(tmp)/'case.exe'
                path.write_text(f'implicit none\ninteger::i,a(9000)\ndo concurrent(i=1:9000)\na(i)={expression}\nend do\nend\n')
                result=subprocess.run([str(COMPILER),'--parallel',str(path),'-o',str(exe)],capture_output=True,text=True,timeout=90)
                self.assertNotEqual(result.returncode,0)
                self.assertIn(diagnostic,result.stderr)

    def test_shadowed_intrinsic_remains_serial(self):
        self.build_run("""implicit none
integer::i,a(9000)
do concurrent(i=1:9000)
 a(i)=iand(i,1)
end do
print *,a(1),a(9000)
contains
pure integer function iand(i,j) result(r)
integer,intent(in)::i,j
r=i+j+10
end function
end
""",'12 9011',reason='serial fallback')

    def test_standalone_example(self):
        self.build_run((ROOT/'tests/programs/xconcurrent_parallel_bits.f90').read_text(),'PASS')


if __name__=='__main__':
    unittest.main()
