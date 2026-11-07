# SPDX-License-Identifier: MIT
"""Parallel harness checks without compiling Fortran or rebuilding rfort."""
import contextlib
import copy
import io
import json
import math
import os
from pathlib import Path
import tempfile
import types
import unittest
from unittest import mock

import run_parallel as runner


def row(mode='parallel', threads=4, n=10000, case='scalar_reduce'):
    return dict(benchmark=case, size='smoke', mode=mode, threads=threads,
                profile=dict(n=n, repetitions=1), status='compiled',
                directory='.', executable='fake', samples=[], warmups=[])


def measurement(r, seconds=.2, trace='', seed=3):
    reference = runner.expected(r['benchmark'], **r['profile'], seed=seed)
    return dict(returncode=0, timeout=False, seconds=seconds + .1, stderr=trace,
                stdout=f'BENCH_SECONDS {seconds}\nBENCH_CHECKSUM {reference}\nBENCH_ERROR 0\n')


class ParallelRunner(unittest.TestCase):
    def test_profiles_and_sources(self):
        for case in runner.CASES:
            source = (runner.HERE / 'parallel' / (case + '.f90')).read_text()
            self.assertIn('do concurrent', source)
            self.assertIn('SPDX-License-Identifier: MIT', source)
            self.assertIn('if(k==1) call start_timer', source)
            for size in runner.PROFILES:
                p = runner.profile(case, size)
                self.assertGreater(p['n'], 0)
                for seed in (3, 20):
                    self.assertTrue(math.isfinite(runner.expected(case, **p, seed=seed)))

    def test_independent_references_against_direct_calculations(self):
        for n in (1, 96, 97, 98, 401):
            for seed in (3, 20):
                repetitions = 3
                steps = n + seed
                residues = [(i+seed) % 97 for i in range(1, steps+1)]
                weight = math.fsum(r/97 for r in residues)
                triangle = sum(range(1, repetitions+1))
                p = (2+3j)
                for _ in range(steps*repetitions):
                    p *= 1j
                z = complex(seed+triangle*weight, 2-.5*triangle*steps)
                matrix = math.fsum((i+seed+j) % 97 / 97
                                   for i in range(1, steps+1) for j in range(1, 5)) * triangle
                values = dict(
                    independent_math=triangle*math.fsum(math.sqrt(r+1)+math.sin(r*.01) for r in residues),
                    scalar_reduce=seed+repetitions*weight,
                    array_reduce=8*(seed+triangle*weight),
                    complex_reduce=4*(abs(z.real)+abs(z.imag))+p.real+2*p.imag,
                    matrix_loop=matrix, section_results=matrix,
                    workspace_calls=sum(k*k for k in range(1,repetitions+1))*sum(
                        (r+j)**2 for r in residues for j in range(1,5)))
                for case, expected in values.items():
                    with self.subTest(case=case, n=n, seed=seed):
                        self.assertTrue(math.isclose(runner.expected(case,n,repetitions,seed), expected, rel_tol=1e-12, abs_tol=1e-9))

    def test_corrupt_and_invalid_measurements_are_rejected(self):
        r = row()
        good = measurement(r)
        self.assertIsNone(runner.validate_measurement(copy.deepcopy(good),r,3))
        cases = [({**good,'timeout':True}, 'run_timeout'),
                 ({**good,'returncode':1}, 'run_failed'),
                 ({**good,'stdout':'BENCH_SECONDS nan\n'}, 'invalid_output'),
                 ({**good,'seconds':.001}, 'invalid_output'),
                 ({**good,'stdout':good['stdout'].replace('BENCH_ERROR 0','BENCH_ERROR 1')}, 'wrong_answer'),
                 ({**good,'stdout':'BENCH_SECONDS .2\nBENCH_CHECKSUM 1\nBENCH_ERROR 0\n'}, 'wrong_answer')]
        for result, status in cases:
            with self.subTest(status=status):
                self.assertEqual(runner.validate_measurement(result,r,3), status)

    def test_dispatch_proof_rejects_missing_or_wrong_thread_counts(self):
        args = types.SimpleNamespace(seed=3, timeout=10)
        for mode, threads, trace, passed in (
            ('parallel',4,'rfort parallel: 4 threads\n',True),
            ('parallel',4,'rfort parallel: 4 threads\nrfort parallel: 4 threads\n',True),
            ('parallel',4,'',False),
            ('parallel',4,'rfort parallel: 2 threads\n',False),
            ('parallel',1,'',True),
            ('serial',1,'',True),
            ('serial',1,'rfort parallel: 2 threads\n',False),
        ):
            r = row(mode,threads)
            with self.subTest(mode=mode, threads=threads, trace=trace), mock.patch.object(
                runner.common,'invoke',return_value=measurement(r,trace=trace)):
                runner.probe_dispatch(r,args)
                self.assertEqual(r['status'], 'compiled' if passed else 'dispatch_failed')

    def test_small_ranges_and_flattened_iteration_counts(self):
        args = types.SimpleNamespace(seed=3, timeout=10)
        for case, n, required in [('scalar_reduce',1024,False),('matrix_loop',1024,True)]:
            r = row(n=n,case=case)
            trace = 'rfort parallel: 4 threads\n' if required else ''
            with mock.patch.object(runner.common,'invoke',return_value=measurement(r,trace=trace)):
                runner.probe_dispatch(r,args)
            self.assertEqual(r['dispatch']['required'],required)
            self.assertEqual(r['status'],'compiled')

    def test_timed_samples_remove_trace_and_record_thread_settings(self):
        r = row()
        args = types.SimpleNamespace(seed=3, timeout=10)
        with mock.patch.dict(os.environ,{'RFORT_PARALLEL_TRACE':'1','RFORT_TIME_PASSES':'1'}), mock.patch.object(
            runner.common,'invoke',return_value=measurement(r)) as invoke:
            runner.measure(r,args,False)
            env = invoke.call_args.args[3]
            self.assertNotIn('RFORT_PARALLEL_TRACE',env)
            self.assertNotIn('RFORT_TIME_PASSES',env)
            self.assertEqual(env['RFORT_NUM_THREADS'],'4')
            self.assertEqual(env['OMP_NUM_THREADS'],'1')
        self.assertEqual(len(r['samples']),1)
        self.assertEqual(len(r['warmups']),0)

    def test_timed_trace_is_a_failure(self):
        r = row()
        with mock.patch.object(runner.common,'invoke',return_value=measurement(r,trace='rfort parallel: 4 threads\n')):
            runner.measure(r,types.SimpleNamespace(seed=3,timeout=10),True)
        self.assertEqual(r['status'],'unexpected_timed_trace')

    def test_speedups_require_success_and_nonzero_times(self):
        rows = [row('serial',1),row('parallel',1),row('parallel',4)]
        for r, seconds in zip(rows,(2.,3.,.5)):
            run = measurement(r,seconds)
            runner.validate_measurement(run,r,3)
            r['samples']=[run]
            runner.summarize(r,.02)
        runner.speedups(rows)
        self.assertEqual(rows[2]['speedup'],4.)
        self.assertAlmostEqual(rows[1]['speedup'],2/3)
        zero = row('serial',1)
        zero.update(status='passed',timing=dict(median=0,reliable_duration=False))
        other = row()
        other.update(status='passed',timing=dict(median=1,reliable_duration=True))
        runner.speedups([zero,other])
        self.assertNotIn('speedup',other)

    def test_failure_is_not_overwritten_by_summary(self):
        r=row()
        r['status']='wrong_answer'
        runner.summarize(r,.02)
        self.assertEqual(r['status'],'wrong_answer')
        self.assertNotIn('timing',r)

    def test_build_checks_native_worker_and_saves_sources(self):
        args=types.SimpleNamespace(optimization='2',checks='none',cold_pool=False,compile_timeout=10)
        for message, status in (
            ('DO CONCURRENT: native parallel worker emitted','compiled'),
            ('DO CONCURRENT: serial fallback: unsupported capture','serial_fallback'),
            ('native parallel worker emitted\nserial fallback: unsupported call','serial_fallback'),
            ('','serial_fallback'),
        ):
            with self.subTest(message=message),tempfile.TemporaryDirectory(prefix='parallel build ') as tmp:
                def invoke(command,cwd,timeout,env):
                    Path(command[-1]).write_text('fake executable')
                    self.assertIn('--parallel',command)
                    self.assertIn('--nocheck',command)
                    return dict(returncode=0,timeout=False,seconds=.1,stdout='',stderr=message)
                with mock.patch.object(runner.common,'invoke',side_effect=invoke):
                    built=runner.build('scalar_reduce','smoke','parallel',{'path':'fake'},args,Path(tmp))
                self.assertEqual(built['status'],status)
                work=Path(built['directory'])
                self.assertEqual((work/'compile.stderr.txt').read_text(),message)
                self.assertIn('warm_pool=.true.',(work/'bench_config.f90').read_text())
                self.assertEqual(set(built['source_hashes']),{'scalar_reduce.f90','bench_support.f90','bench_config.f90'})

    def test_build_failure_and_cold_pool_config(self):
        args=types.SimpleNamespace(optimization='0',checks='default',cold_pool=True,compile_timeout=10)
        for timeout,status in ((True,'compile_timeout'),(False,'compile_failed')):
            with tempfile.TemporaryDirectory() as tmp, mock.patch.object(runner.common,'invoke',return_value=dict(
                returncode=1,timeout=timeout,seconds=.1,stdout='',stderr='error')) as invoke:
                built=runner.build('scalar_reduce','smoke','serial',{'path':'fake'},args,Path(tmp))
                self.assertEqual(built['status'],status)
                command=invoke.call_args.args[0]
                self.assertNotIn('--parallel',command)
                self.assertNotIn('--nocheck',command)
                self.assertIn('warm_pool=.false.',(Path(built['directory'])/'bench_config.f90').read_text())

    def test_mock_sweep_writes_reports_and_measures_rotating_order(self):
        with tempfile.TemporaryDirectory() as tmp:
            output=Path(tmp)/'results'
            calls=[]
            def build(case,size,mode,compiler,args,destination):
                r=row(mode,case=case)
                r['size']=size
                return r
            def measure(r,args,warmup):
                calls.append((warmup,r['mode'],r['threads']))
                result=measurement(r,seconds=1/r['threads'])
                runner.validate_measurement(result,r,args.seed)
                r['warmups' if warmup else 'samples'].append(result)
            with mock.patch.object(runner.common,'compiler_info',return_value={'available':True,'path':'fake','sha256':'abc'}), \
                 mock.patch.object(runner,'build',side_effect=build) as builds, \
                 mock.patch.object(runner,'probe_dispatch'), mock.patch.object(runner,'measure',side_effect=measure), \
                 contextlib.redirect_stdout(io.StringIO()):
                code=runner.main(['--size','smoke','--bench','scalar_reduce','--runs','2','--warmups','1','--output',str(output)])
            self.assertEqual(code,0)
            self.assertEqual(builds.call_count,2)
            report=json.loads((output/'report.json').read_text())
            self.assertEqual(len(report['results']),4)
            self.assertTrue(all(len(r['samples'])==2 and len(r['warmups'])==1 for r in report['results']))
            measured=[(mode,t) for warmup,mode,t in calls if not warmup]
            self.assertEqual(measured[:4],[('serial',1),('parallel',1),('parallel',2),('parallel',4)])
            self.assertEqual(measured[4:],[('parallel',1),('parallel',2),('parallel',4),('serial',1)])
            self.assertIn('| scalar_reduce |',(output/'summary.md').read_text())

    def test_fallback_is_recorded_as_failure_without_running_it(self):
        with tempfile.TemporaryDirectory() as tmp:
            def build(case,size,mode,*args):
                r=row(mode)
                r['status']='serial_fallback' if mode=='parallel' else 'compile_failed'
                return r
            with mock.patch.object(runner.common,'compiler_info',return_value={'available':True,'path':'fake'}), \
                 mock.patch.object(runner,'build',side_effect=build), mock.patch.object(runner,'probe_dispatch') as probe, \
                 mock.patch.object(runner,'measure') as measure, contextlib.redirect_stdout(io.StringIO()):
                code=runner.main(['--size','smoke','--bench','scalar_reduce','--output',tmp])
            self.assertEqual(code,1)
            probe.assert_not_called()
            measure.assert_not_called()
            report=json.loads((Path(tmp)/'report.json').read_text())
            self.assertEqual([r['status'] for r in report['results']],['compile_failed']+['serial_fallback']*3)

    def test_rejects_invalid_arguments_and_existing_output(self):
        with tempfile.TemporaryDirectory() as tmp:
            (Path(tmp)/'keep.txt').write_text('keep')
            for args in (['--runs','0'],['--warmups','-1'],['--threads','65'],['--seed','2'],
                         ['--timeout','nan'],['--bench','missing'],['--output',tmp]):
                with self.subTest(args=args),contextlib.redirect_stderr(io.StringIO()),self.assertRaises(SystemExit):
                    runner.main(args)
            self.assertEqual((Path(tmp)/'keep.txt').read_text(),'keep')


if __name__=='__main__':
    unittest.main()
