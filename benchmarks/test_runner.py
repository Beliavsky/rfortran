# SPDX-License-Identifier: MIT
# Copyright (c) 2026 Beliavsky
"""Harness tests; no Fortran compiler or compiler build is required."""
import contextlib
import io
import json
import math
from pathlib import Path
import sys
import tempfile
import unittest
from unittest import mock

import run as runner


class RunnerTests(unittest.TestCase):
    def setUp(self):
        self.cases = json.loads((runner.HERE/'manifest.json').read_text())['benchmarks']

    def test_all_profiles_have_finite_independent_references(self):
        self.assertEqual(len({c['name'] for c in self.cases}), len(self.cases))
        for c in self.cases:
            self.assertTrue((runner.HERE/c['source']).is_file())
            for p in c['profiles'].values():
                for seed in (3, 20):
                    self.assertTrue(math.isfinite(runner.expected_result(c['reference'], **p, seed=seed)))

    def test_residue_and_dot_reference_against_direct_sums(self):
        for n in (1, 96, 97, 98, 20000):
            self.assertEqual(runner.residue_sum(n, 3, 97), sum((i+3)%97 for i in range(1,n+1)))
            x = [(i+3)%97/97 for i in range(1,n+1)]
            y = [(i+6)%89/89 for i in range(1,n+1)]
            value = 0.
            for _ in range(4):
                y[0] += .0001
                value += math.fsum(a*b for a,b in zip(x,y))
            self.assertAlmostEqual(runner.expected_result('dot',n,4,3),value,places=9)

    def test_matrix_and_stencil_references(self):
        n, seed, repeats = 5, 3, 2
        a = [[(i+1+seed)/(n+seed) for j in range(n)] for i in range(n)]
        b = [[(j+1)/(n+seed) for j in range(n)] for i in range(n)]
        a[0][0] += repeats*.000001
        checksum = sum(sum(a[i][k]*b[k][j] for k in range(n)) for j in range(n) for i in range(n))
        self.assertAlmostEqual(runner.expected_result('matrix',n,repeats,seed),checksum)
        u = [[seed*math.sin(math.pi*i/(n-1))*math.sin(math.pi*j/(n-1)) for j in range(n)] for i in range(n)]
        for _ in range(repeats):
            v = [[0.]*n for _ in range(n)]
            for i in range(1,n-1):
                for j in range(1,n-1):
                    v[i][j] = .25*(u[i-1][j]+u[i+1][j]+u[i][j-1]+u[i][j+1])
            u = v
        self.assertAlmostEqual(runner.expected_result('stencil',n,repeats,seed),sum(map(sum,u)))

    def test_output_parser_rejects_invalid_measurements(self):
        good = 'BENCH_SECONDS 1D-2\nBENCH_CHECKSUM 3.0\nBENCH_ERROR 0.0\n'
        self.assertEqual(runner.parse_output(good)['BENCH_SECONDS'], .01)
        for bad in (good+'BENCH_SECONDS 1\n', good.replace('3.0','NaN'),
                    good.replace('3.0','Inf'), good.replace('1D-2','-1'),
                    good.replace('BENCH_ERROR 0.0\n',''),good.replace('3.0','3.0 extra')):
            with self.subTest(output=bad), self.assertRaises(ValueError):
                runner.parse_output(bad)

    def test_correctness_uses_checksum_and_element_error(self):
        case = self.cases[0]
        p = case['profiles']['smoke']
        expected = runner.expected_result(case['reference'], **p,seed=3)
        values = dict(BENCH_SECONDS=.1,BENCH_CHECKSUM=expected,BENCH_ERROR=0.)
        self.assertTrue(runner.validate(values,case,p,3)[0])
        self.assertFalse(runner.validate(dict(values,BENCH_CHECKSUM=expected+1),case,p,3)[0])
        self.assertFalse(runner.validate(dict(values,BENCH_ERROR=1),case,p,3)[0])
        large = case['profiles']['large']
        expected = runner.expected_result(case['reference'], **large,seed=3)
        self.assertFalse(runner.validate(dict(values,BENCH_CHECKSUM=expected+1),case,large,3)[0])

    def test_batch_wrappers_cannot_trigger_a_compiler_rebuild(self):
        with tempfile.TemporaryDirectory() as tmp, mock.patch.object(runner,'invoke') as invoke:
            wrapper = Path(tmp)/'compiler.bat'
            wrapper.write_text('@echo off\nexit /b 0\n')
            result = runner.compiler_info('rfort',str(wrapper),Path(tmp),{})
            self.assertFalse(result['available'])
            invoke.assert_not_called()

    def test_subprocess_errors_and_timeouts_are_distinct(self):
        with tempfile.TemporaryDirectory() as tmp:
            result = runner.invoke([sys.executable,'-c','raise SystemExit(2)'],Path(tmp),5,{})
            self.assertEqual(result['returncode'],2)
            self.assertFalse(result['timeout'])
            result = runner.invoke([sys.executable,'-c','import time; time.sleep(2)'],Path(tmp),.1,{})
            self.assertTrue(result['timeout'])
            result = runner.invoke([str(Path(tmp)/'missing.exe')],Path(tmp),5,{})
            self.assertIsNone(result['returncode'])
            self.assertFalse(result['timeout'])

    def test_missing_compiler_is_reported_and_old_reports_preserved(self):
        with tempfile.TemporaryDirectory() as tmp:
            output = Path(tmp)/'report'
            args = ['--compilers','rfort','--rfort',str(Path(tmp)/'missing.exe'),
                    '--bench','integer_sum','--size','smoke','--runs','1','--warmups','0','--output',str(output)]
            with contextlib.redirect_stdout(io.StringIO()):
                self.assertEqual(runner.main(args),1)
            report = json.loads((output/'report.json').read_text())
            self.assertEqual(report['results'][0]['status'],'unavailable')
            self.assertTrue((output/'summary.md').is_file())
            with contextlib.redirect_stderr(io.StringIO()), self.assertRaises(SystemExit):
                runner.main(args)
            self.assertEqual(json.loads((output/'report.json').read_text()),report)
            args[args.index('--output')+1] = str(Path(tmp)/'allowed')
            with contextlib.redirect_stdout(io.StringIO()):
                self.assertEqual(runner.main(args+['--allow-failures']),0)

    def test_compile_failure_retains_command_and_logs(self):
        args = runner.parser().parse_args(['--rfort-flag=-O0'])
        case = self.cases[0]
        compiler = dict(name='rfort',path='example compiler.exe',available=True)
        failed = dict(command=[],returncode=1,timeout=False,seconds=.1,stdout='',stderr='syntax error')
        with tempfile.TemporaryDirectory() as tmp, mock.patch.object(runner,'invoke',return_value=failed) as invoke:
            r = runner.compile_case(case,compiler,case['profiles']['smoke'],args,Path(tmp),{})
            self.assertEqual(r['status'],'compile_failed')
            command = invoke.call_args.args[0]
            self.assertEqual(command[:3],['example compiler.exe','-O0','bench_config.f90'])
            self.assertEqual((Path(r['directory'])/'compile.stderr.txt').read_text(),'syntax error')

    def test_explicit_optimization_replaces_the_default(self):
        args = runner.parser().parse_args(['--rfort-flag=-O0','--gfortran-flag=-O3'])
        self.assertEqual(runner.compiler_flags(args,'rfort'),['-O0'])
        self.assertEqual(runner.compiler_flags(args,'gfortran'),['-O3'])
        args = runner.parser().parse_args(['--rfort-flag=--quiet'])
        self.assertEqual(runner.compiler_flags(args,'rfort'),['-O2','--quiet'])
        args = runner.parser().parse_args(['--no-default-flags'])
        self.assertEqual(runner.compiler_flags(args,'gfortran'),[])

    def test_failed_run_never_receives_a_timing_summary(self):
        case = self.cases[0]
        result = dict(status='compiled',profile=case['profiles']['smoke'],directory='.',
                      executable='fake',samples=[],warmups=[])
        run = dict(returncode=0,timeout=False,seconds=.1,
                   stdout='BENCH_SECONDS 0.1\nBENCH_CHECKSUM -123\nBENCH_ERROR 0\n',stderr='')
        args = runner.parser().parse_args([])
        with mock.patch.object(runner,'invoke',return_value=run):
            runner.sample(result,case,args,{},False)
        self.assertEqual(result['status'],'wrong_answer')
        runner.summarize(result,.02)
        self.assertNotIn('timing',result)

    def test_statistics_exclude_warmups_and_ratios_exclude_failures(self):
        result = dict(status='compiled',warmups=[dict(seconds=999)], samples=[
            dict(seconds=t+1,values={'BENCH_SECONDS':t}) for t in (1.,2.,3.)])
        runner.summarize(result,.02)
        self.assertEqual(result['timing']['median'],2.)
        self.assertEqual(result['timing']['median_wall'],3.)
        report = dict(created_utc='test',settings=dict(size='small',seed=3,runs=3),
                      benchmarks=[dict(name='kernel')],compilers={},results=[
                          dict(result,benchmark='kernel',compiler='rfort'),
                          dict(result,benchmark='kernel',compiler='gfortran')])
        self.assertIn('| 2 | 2 | 1.000 |',runner.markdown(report))
        report['results'][0]['status']='wrong_answer'
        self.assertIn('| — | 2 | — |',runner.markdown(report))

    def test_impossible_kernel_time_cannot_produce_a_speed_comparison(self):
        case = self.cases[0]
        profile = case['profiles']['smoke']
        expected = runner.expected_result(case['reference'], **profile,seed=3)
        result = dict(status='compiled',profile=profile,directory='.',executable='fake',samples=[],warmups=[])
        run = dict(returncode=0,timeout=False,seconds=.1,
                   stdout=f'BENCH_SECONDS 99\nBENCH_CHECKSUM {expected}\nBENCH_ERROR 0\n',stderr='')
        with mock.patch.object(runner,'invoke',return_value=run):
            runner.sample(result,case,runner.parser().parse_args([]),{},False)
        self.assertEqual(result['status'],'invalid_output')
        runner.summarize(result,.02)
        self.assertNotIn('timing',result)


if __name__ == '__main__':
    unittest.main()
