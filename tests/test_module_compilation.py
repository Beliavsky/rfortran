"""Separate compilation, persistent module dependencies, and cross-object state."""
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from test_record_read import COMPILER


class ModuleCompilation(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory(prefix='rfort modules (A&B) ')
        self.addCleanup(self.tmp.cleanup)
        self.directory = Path(self.tmp.name)

    def source(self, name, text):
        path = self.directory / (name + '.f90')
        path.write_text(text, encoding='utf-8')
        return path

    def command(self, *args):
        return subprocess.run([str(COMPILER), *map(str, args)], cwd=self.directory,
                              text=True, capture_output=True, timeout=90)

    def success(self, *args):
        result = self.command(*args)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertEqual(result.stderr, '')
        return result

    def obj(self, name):
        return self.directory / (name + ('.obj' if os.name == 'nt' else '.o'))

    def run_objects(self, names, expected):
        exe = self.directory / ('program.exe' if os.name == 'nt' else 'program')
        self.success(*[self.obj(n) for n in names], '-o', exe)
        result = subprocess.run([exe], capture_output=True, text=True, timeout=15)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stderr, '')
        self.assertEqual(result.stdout.split(), expected.split())

    def test_constants_and_transitive_dependencies_without_sources(self):
        kind = self.source('kind', '''module kind_mod
use, intrinsic :: iso_fortran_env, only: real64
implicit none
private
public :: dp, label
integer,parameter :: dp=real64
character(*),parameter :: label='separate'
end module
''')
        self.success('-c', kind)
        self.assertTrue((self.directory / 'kind_mod.rmod').is_file())
        kind.unlink()
        m = self.source('m', '''module m
use kind_mod, only: dp, label
implicit none
real(dp),parameter :: value=2.5_dp
end module
''')
        self.success('-c', m)
        m.unlink()
        main = self.source('main', '''use m, only: value, label
implicit none
if (value /= 2.5d0) stop 1
print *, label, value
end
''')
        self.success('-c', main)
        for interface in self.directory.glob('*.rmod'):
            interface.unlink()
        self.run_objects(['kind', 'm', 'main'], 'separate 2.5')

    def test_formatted_implied_do_runtime_dummy_bounds(self):
        module = self.source('lags', '''module lags_mod
implicit none
contains
subroutine print_lags(nlags, first, step)
integer,intent(in) :: nlags, first, step
integer :: i,j
write (*,"(/,'ACF_lag',100i8)") (i,i=1,nlags)
write (*,'(100(i0,1x,a,1x))') (i,'lag',i=nlags,first,step)
write (*,'(100i8)') ((10*i+j,j=1,i),i=1,nlags)
end subroutine
end module
''')
        for opt in ['-O0', '-O2']:
            with self.subTest(opt=opt):
                self.success(opt, '-c', module)
                main = self.source('main', '''use lags_mod
implicit none
integer :: n
n=3
call print_lags(n,1,-1)
call print_lags(0,1,-1)
end
''')
                self.success(opt, '-c', main)
                self.run_objects(['lags', 'main'],
                                 'ACF_lag 1 2 3 3 lag 2 lag 1 lag 11 21 22 31 32 33 ACF_lag')

    def test_procedures_and_shared_state_across_objects(self):
        m = self.source('counter', '''module counter
implicit none
integer :: total=10
contains
integer function next() result(n)
integer :: calls=0
calls=calls+1
total=total+1
n=calls
end function
end module
''')
        self.success('-O0', '-c', m)
        m.unlink()
        for name in ['first', 'second']:
            wrapper = self.source(name, f'''subroutine {name}()
use counter, only: next
implicit none
integer :: n
n=next()
if (n/={1 if name == "first" else 2}) stop 4
end subroutine
''')
            self.success('-O0' if name == 'first' else '-O2', '-c', wrapper)
        main = self.source('main', '''use counter, only: total, next
implicit none
integer :: n
call first()
call second()
n=next()
if (n/=3 .or. total/=13) stop 3
print *, n,total
end
''')
        self.success('-c', main)
        self.run_objects(['counter', 'first', 'second', 'main'], '3 13')

    def test_saved_uninitialized_state_shared_across_objects(self):
        module = self.source('cache', '''module cache_mod
implicit none
contains
subroutine cache(set_value,value)
logical,intent(in) :: set_value
integer,intent(inout) :: value
integer,save :: stored
if(set_value)then
  stored=value
else
  value=stored
end if
end subroutine
end module
''')
        self.success('-c', module)
        module.unlink()
        first = self.source('first', '''subroutine first()
use cache_mod
implicit none
integer :: n
n=42
call cache(.true.,n)
end subroutine
''')
        self.success('-O0', '-c', first)
        main = self.source('main', '''use cache_mod
implicit none
integer :: n
call first()
n=0
call cache(.false.,n)
print *,n
end
''')
        self.success('-O2', '-c', main)
        self.run_objects(['cache', 'first', 'main'], '42')

    def test_generic_derived_array_character_and_keyword_interfaces(self):
        m = self.source('features', '''module features
implicit none
private
public :: item, make, shift, suffix
type :: item
integer :: n
character(3) :: tag
end type item
interface make
module procedure make_item
end interface make
contains
function make_item(n) result(x)
integer,intent(in) :: n
type(item) :: x
x%n=n
x%tag='abc'
end function
subroutine shift(a,amount)
real(8),intent(inout) :: a(:,:)
real(8),intent(in) :: amount
a=a+amount
end subroutine
function suffix(text) result(out)
character(*),intent(in) :: text
character(len(text)+1) :: out
out=text//'!'
end function
end module
''')
        self.success('-c', m)
        m.unlink()
        main = self.source('main', '''use features, only: record=>item, make, shift, suffix
implicit none
type(record) :: x
real(8),allocatable :: a(:,:)
integer :: n
n=2
allocate(a(n,3))
a=1.0d0
call shift(amount=2.0d0,a=a)
x=make(n=7)
if (any(a/=3.0d0)) stop 1
if (x%n/=7 .or. x%tag/='abc') stop 2
if (suffix('ok')/='ok!') stop 3
print *, x%n,x%tag,suffix('ok'),sum(a)
end
''')
        self.success('-c', main)
        self.run_objects(['features', 'main'], '7 abcok! 18.0')

    def test_unused_module_variables_and_runtime_arrays_shared(self):
        m = self.source('state', '''module state
implicit none
integer :: value
integer,allocatable :: a(:,:)
character(4) :: text='abcd'
end module
''')
        self.success('-c', m)
        fill = self.source('fill', '''subroutine fill()
use state
implicit none
integer :: n
n=3
allocate(a(2,n))
a=4
value=17
text='done'
end subroutine
''')
        self.success('-c', fill)
        main = self.source('main', '''use state
implicit none
call fill()
if (.not.allocated(a)) stop 1
if (size(a)/=6 .or. sum(a)/=24) stop 2
if (value/=17 .or. text/='done') stop 3
print *,value,sum(a),text
deallocate(a)
end
''')
        self.success('-c', main)
        self.run_objects(['state', 'fill', 'main'], '17 24 done')

    def test_logical_real_and_character_module_storage(self):
        m = self.source('state', '''module state
implicit none
logical :: flag=.false.
real(8) :: value=1.0d0
character(3),allocatable :: words(:,:)
end module
''')
        self.success('-c', m)
        fill = self.source('fill', '''subroutine fill()
use state
implicit none
integer :: n
flag=.true.
value=2.5d0
n=2
allocate(words(n,n))
words='ok'
end subroutine
''')
        self.success('-c', fill)
        main = self.source('main', '''use state
implicit none
call fill()
if (.not.flag .or. value/=2.5d0) stop 1
if (any(words/='ok')) stop 4
print *,flag,value,words(1,1)
end
''')
        self.success('-c', main)
        self.run_objects(['state', 'fill', 'main'], 'T 2.5 ok')

    def test_module_paths_and_missing_dependency(self):
        module = self.source('k', 'module k\ninteger,parameter::n=4\nend module\n')
        moddir = self.directory / 'module files'
        self.success('-J', moddir, '-c', module)
        main = self.source('main', 'use k,only:n\nprint *,n\nend\n')
        failure = self.command('-c', main)
        self.assertNotEqual(failure.returncode, 0)
        self.assertIn('k.rmod', failure.stderr)
        self.success('-I' + str(moddir), '-c', main)
        self.run_objects(['k', 'main'], '4')

    def test_check_and_interpreter_load_interfaces(self):
        m = self.source('m', 'module m\ninteger,parameter::n=42\nend module\n')
        self.success('-c', m)
        m.unlink()
        main = self.source('main', 'use m\nprint *,n\nend\n')
        self.assertIn('No problems found.', self.success('check', main).stdout)
        self.assertEqual(self.success('run', main).stdout.split(), ['42'])

    def test_diagnostics_keep_defining_source_line(self):
        m = self.source('m', '''module m
implicit none
contains
integer function f(n) result(y)
integer,intent(in)::n
integer::x
if (n>0) x=3
y=x
end function
end module
''')
        self.success('-c', m)
        m.unlink()
        main = self.source('main', 'use m\nprint *,f(0)\nend\n')
        self.success('-c', main)
        exe = self.directory / ('bad.exe' if os.name == 'nt' else 'bad')
        self.success(self.obj('m'), self.obj('main'), '-o', exe)
        result = subprocess.run([exe], capture_output=True, text=True, timeout=15)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('m.f90:8', result.stderr)
        result = self.command('run', main)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('m.f90:8', result.stderr)

    def test_circular_dependencies_and_current_source_precedence(self):
        a = self.source('a', 'module a\nuse b\nend module\nmodule b\nuse a\nend module\n')
        failure = self.command('-c', a)
        self.assertNotEqual(failure.returncode, 0)
        self.assertIn('circular module dependency', failure.stderr)
        self.assertFalse((self.directory / 'a.rmod').exists())
        a.write_text('module a\ninteger,parameter::n=4\nend module\n')
        self.success('-c', a)
        a.write_text('module a\ninteger,parameter::n=5\nend module\n')
        main = self.source('main', 'use a\nprint *,n\nend\n')
        self.success('-c', a, main)
        self.run_objects(['main'], '5')

    def test_stale_and_incompatible_interfaces(self):
        k = self.source('k', 'module k\ninteger,parameter::n=4\nend module\n')
        m = self.source('m', 'module m\nuse k\ninteger,parameter::twice=2*n\nend module\n')
        main = self.source('main', 'use m\nprint *,twice\nend\n')
        self.success('-c', k)
        self.success('-c', m)
        k.write_text('module k\ninteger,parameter::n=5\nend module\n')
        self.success('-c', k)
        failure = self.command('-c', main)
        self.assertNotEqual(failure.returncode, 0)
        self.assertIn('stale module interface', failure.stderr)
        self.success('-c', m)
        self.success('-c', main)
        self.run_objects(['k', 'm', 'main'], '10')
        path = self.directory / 'm.rmod'
        interface = json.loads(path.read_text())
        interface['rfort_module_format'] = 999
        path.write_text(json.dumps(interface))
        failure = self.command('-c', main)
        self.assertNotEqual(failure.returncode, 0)
        self.assertIn('incompatible module interface', failure.stderr)

    def test_failed_build_preserves_interface_and_visibility(self):
        m = self.source('m', 'module m\nprivate\ninteger,parameter::secret=4\nend module\n')
        self.success('-c', m)
        path = self.directory / 'm.rmod'
        old = path.read_bytes()
        main = self.source('main', 'use m,only:secret\nprint *,secret\nend\n')
        failure = self.command('-c', main)
        self.assertNotEqual(failure.returncode, 0)
        self.assertIn('not a public entity', failure.stderr)
        m.write_text('module m\ncontains\nsubroutine bad()\nimplicit none\nprint *,missing\nend subroutine\nend module\n')
        self.assertNotEqual(self.command('-c', m).returncode, 0)
        self.assertEqual(path.read_bytes(), old)


if __name__ == '__main__':
    unittest.main(verbosity=2)
