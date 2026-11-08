"""Private-index serial DO loops inside native parallel iterations."""
import unittest
from test_record_read import ROOT
import test_concurrent_parallel_captures


class InnerDo(unittest.TestCase):
    check_source = test_concurrent_parallel_captures.ParallelCaptures.check_source

    def test_example(self):
        self.check_source((ROOT/'tests/programs/xconcurrent_parallel_inner_do.f90').read_text())

    def test_signed_and_runtime_strides(self):
        for step, start, end in [('2','1','5'), ('-2','5','1'), ('stride','5','1')]:
            with self.subTest(step=step):
                self.check_source(self.source(f'''total=0
do j={start},{end},{step}
 total=total+j*i
end do
a(i)=total''', 'local(j,total)', '9*i'))

    def test_empty_inner_loop(self):
        self.check_source(self.source('''total=7
do j=4,1
 total=total+1
end do
a(i)=total''', 'local(j,total)', '7'))

    def test_bounds_evaluated_once_per_inner_loop(self):
        source=self.source('''total=3
do j=1,total
 total=0
end do
a(i)=j''', 'local_init(j) local(total)', '4')
        source=source.replace('stride=-2', 'stride=-2\nj=42')
        source=source.replace('print *,"PASS"', 'if(j/=42) stop 2\nprint *,"PASS"')
        self.check_source(source)

    def test_nested_loops_local_init_and_reduction(self):
        self.check_source('''implicit none
integer::i,j,k,total,a(9001)
integer(kind=8)::s
total=5
s=0
do concurrent(i=1:9001) local(j,k) local_init(total) reduce(+:s)
 do j=1,3
  do k=1,2
   total=total+i
  end do
 end do
 a(i)=total
 s=s+total
end do
do i=1,9001
 if(a(i)/=5+6*i) stop 1
end do
if(s/=5_8*9001+3_8*9001*9002) stop 2
if(total/=5) stop 3
print *,"PASS"
end
''')

    def test_runtime_private_workspace(self):
        self.check_source('''implicit none
integer::a(9001),i
call compute(3,a)
do i=1,size(a)
 if(a(i)/=6*i) stop 1
end do
print *,"PASS"
contains
subroutine compute(n,a)
integer,intent(in)::n
integer,intent(out)::a(:)
integer::i,j,work(n)
do concurrent(i=1:size(a)) local(j,work)
 do j=1,n
  work(j)=j*i
 end do
 a(i)=sum(work)
end do
end subroutine
end
''')

    def test_initialization_resets_between_outer_iterations(self):
        self.check_source(self.source('''do j=1,1
 if(mod(i,2)==1) total=i
end do
a(i)=total''', 'local(j,total)', 'i'), runtime_error='uninitialized')

    def test_nonprivate_index_falls_back(self):
        self.check_source(self.source('''total=0
do j=1,3
 total=total+i
end do
a(i)=total''', 'local(total)', '3*i'), reason='inner DO index requires')

    def test_shared_write_proof_is_retained(self):
        self.check_source(self.source('''do j=1,1
 a(i+j-1)=i
end do''', 'local(j)', 'i'), reason='independent array-element writes require')

    def test_exit_retains_serial_fallback(self):
        self.check_source(self.source('''total=0
do j=1,4
 if(j==3) exit
 total=total+i
end do
a(i)=total''', 'local(j,total)', '2*i'), reason='body requires')

    @staticmethod
    def source(body, clause, expected):
        return f'''implicit none
integer::i,j,total,a(9001),stride
stride=-2
do concurrent(i=1:9001) {clause}
{body}
end do
do i=1,size(a)
 if(a(i)/={expected}) stop 1
end do
print *,"PASS"
end
'''


if __name__ == '__main__':
    unittest.main()
