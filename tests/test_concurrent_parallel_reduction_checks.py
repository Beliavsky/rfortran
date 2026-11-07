"""Reduction metadata is not a value read; actual inputs remain checked."""
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

from test_record_read import COMPILER


class ReductionChecks(unittest.TestCase):
    def build(self, source, work, optimization):
        path=work/'case.f90'
        path.write_text(source)
        executable=work/'case.exe'
        result=subprocess.run([str(COMPILER),'-O'+optimization,'--parallel','--parallel-report',
                               '--debug-ir',str(path),'-o',str(executable)],cwd=work,
                              capture_output=True,text=True,timeout=120)
        self.assertEqual(result.returncode,0,result.stderr)
        self.assertIn('native parallel worker emitted',result.stderr)
        self.assertNotIn('serial fallback',result.stderr)
        return executable,result.stdout

    def test_worker_result_metadata_has_no_array_scan(self):
        source="""implicit none
integer::i,n
real(kind=kind(0d0))::s(8)
n=9001
s=3.0d0
do concurrent(i=1:n) reduce(+:s)
s=s+1.0d0
end do
if(any(abs(s-9004.0d0)>1d-8)) stop 1
print *, 'PASS'
end
"""
        with tempfile.TemporaryDirectory() as tmp:
            for optimization in ('0','2'):
                executable,ir=self.build(source,Path(tmp),optimization)
                self.assertIn('@parallel_iteration(',ir)
                self.assertNotIn('@init_scan_',ir)
                for threads in (1,2,4):
                    result=subprocess.run([str(executable)],cwd=tmp,env=dict(os.environ,RFORT_NUM_THREADS=str(threads)),
                                          capture_output=True,text=True,timeout=30)
                    self.assertEqual(result.returncode,0,result.stderr)
                    self.assertEqual(result.stdout.split(),['PASS'])

    def test_reduction_inputs_keep_initialization_and_bounds_checks(self):
        for failure in ('uninitialized','bounds'):
            source="""implicit none
integer::i,n,offset
real,allocatable::a(:)
real::s(8)
n=9001
offset=OFFSET
allocate(a(n))
INITIALIZE
s=3.0
do concurrent(i=1:n) reduce(+:s)
s=s+a(i+offset)
end do
print *,s
end
""".replace('OFFSET','1' if failure=='bounds' else '0').replace('INITIALIZE',
                'a=1.0' if failure=='bounds' else 'do i=1,n\nif(modulo(i,2)==0) a(i)=1.0\nend do')
            with self.subTest(failure=failure),tempfile.TemporaryDirectory() as tmp:
                for optimization in ('0','2'):
                    executable,_=self.build(source,Path(tmp),optimization)
                    for threads in (1,2,4):
                        result=subprocess.run([str(executable)],cwd=tmp,
                                              env=dict(os.environ,RFORT_NUM_THREADS=str(threads)),
                                              capture_output=True,text=True,timeout=30)
                        self.assertNotEqual(result.returncode,0,result.stdout)
                        self.assertIn('uninitialized' if failure=='uninitialized' else 'bound',result.stderr.lower())
                        if failure=='uninitialized':
                            self.assertIn('case.f90:',result.stderr)


if __name__=='__main__':
    unittest.main()
