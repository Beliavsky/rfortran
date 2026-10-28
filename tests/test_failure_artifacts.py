"""Exercise failure retention, including failures swallowed by unittest subtests."""
import contextlib
import io
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

from failure_artifacts import FailureArtifacts, digest
import stress_native_output
import test_deallocate


def fake_build(source, directory, **kwargs):
    (directory/'read0.obj').write_bytes(b'original object')
    (directory/'read0.exe').write_bytes(b'original executable')
    return [[sys.executable, '-c', 'pass']]


class FailureRetention(unittest.TestCase):
    def setUp(self):
        temporary=tempfile.TemporaryDirectory(prefix='artifact-tests-')
        self.addCleanup(temporary.cleanup)
        self.work=Path(temporary.name)
        self.root=self.work/'failures'
        env=patch.dict(os.environ, RFORT_TEST_FAILURE_DIR=str(self.root))
        env.start()
        self.addCleanup(env.stop)

    def retained(self):
        folders=list(self.root.iterdir())
        self.assertEqual(len(folders),1)
        return folders[0],json.loads((folders[0]/'failure.json').read_text())

    def test_zero_exit_empty_stdout_keeps_original_files(self):
        case=test_deallocate.Deallocate('test_fixture')
        with patch('test_deallocate.build_commands', side_effect=fake_build):
            with self.assertRaisesRegex(AssertionError,'Failure artifacts:'):
                case.check('print *,42\nend\n','42',reference=False,data='input\n')
        folder,report=self.retained()
        self.assertEqual((folder/'example.f90').read_text(),'print *,42\nend\n')
        self.assertEqual((folder/'read0.exe').read_bytes(),b'original executable')
        self.assertEqual((folder/'read0.obj').read_bytes(),b'original object')
        self.assertEqual((folder/'stdin.txt').read_bytes(),b'input\n')
        self.assertFalse(Path(report['original_directory']).exists())
        command=report['commands'][0]
        self.assertEqual(command['returncode'],0)
        self.assertEqual((folder/command['stdout_file']).read_text(),'')
        self.assertEqual(report['files_sha256']['read0.obj'],digest(folder/'read0.obj'))
        self.assertEqual(report['failure_events'][0]['command_index'],0)
        self.assertEqual(report['label'],case.id())

    def test_compiler_failure_keeps_diagnostics_and_relocated_command(self):
        case=test_deallocate.Deallocate('test_fixture')
        failed=subprocess.CompletedProcess([],2,'compiler output\n','compiler error\n')
        with patch('failure_artifacts.subprocess.run',return_value=failed):
            with self.assertRaises(AssertionError):
                case.check('end\n','',reference=False)
        folder,report=self.retained()
        command=report['commands'][0]
        self.assertEqual(command['returncode'],2)
        self.assertEqual((folder/command['stderr_file']).read_text(),'compiler error\n')
        self.assertIn(str(folder/'example.f90'),command['retained_command'])
        self.assertNotIn(str(folder/'example.f90'),command['command'])

    def test_timeout_keeps_partial_raw_output(self):
        case=self.work/'case'
        case.mkdir()
        error=subprocess.TimeoutExpired(['missing'],.1,output=b'partial\xff',stderr=b'error')
        with self.assertRaises(subprocess.TimeoutExpired):
            with FailureArtifacts(case,'timeout') as recorder:
                with patch('failure_artifacts.subprocess.run',side_effect=error):
                    recorder.run(['missing'],capture_output=True,timeout=.1)
        folder,report=self.retained()
        command=report['commands'][0]
        self.assertEqual((folder/command['stdout_file']).read_bytes(),b'partial\xff')
        self.assertIn('TimeoutExpired',command['exception'])

    def test_subtest_failure_is_saved_after_later_success(self):
        class Inner(unittest.TestCase):
            def runTest(inner):
                test_deallocate.Deallocate.check(inner,'end\n','42',reference=False)
        def commands(*args,**kwargs):
            return [[sys.executable,'-c','pass'],[sys.executable,'-c','print(42)']]
        result=unittest.TestResult()
        with patch('test_deallocate.build_commands',side_effect=commands):
            Inner().run(result)
        self.assertEqual(len(result.failures),1)
        folder,report=self.retained()
        self.assertEqual(len(report['commands']),2)
        self.assertEqual((folder/report['commands'][0]['stdout_file']).read_text(),'')
        self.assertEqual((folder/report['commands'][1]['stdout_file']).read_text(),'42\n')

    def test_success_leaves_no_retained_directory(self):
        case=test_deallocate.Deallocate('test_fixture')
        with patch('test_deallocate.build_commands',return_value=[[sys.executable,'-c','print(42)']]):
            case.check('end\n','42',reference=False)
        self.assertFalse(self.root.exists())

    def test_outer_temporary_working_directory_survives_cleanup(self):
        with tempfile.TemporaryDirectory(prefix='outer-artifact-cwd-') as tmp:
            outer=Path(tmp)
            (outer/'input.txt').write_text('data\n')
            case=self.work/'case'
            case.mkdir()
            with FailureArtifacts(case,'cwd') as recorder:
                recorder.run([sys.executable,'-c','pass'],cwd=outer,capture_output=True,text=True)
                recorder.run([sys.executable,'-c','pass',outer/'input.txt'],cwd=outer,capture_output=True,text=True)
                recorder.failed('injected mismatch')
        folder,report=self.retained()
        cwd=Path(report['commands'][0]['retained_cwd'])
        self.assertTrue(cwd.is_relative_to(folder))
        self.assertEqual((cwd/'input.txt').read_text(),'data\n')
        self.assertEqual(len(report['working_directories']),1)
        self.assertEqual(report['commands'][1]['retained_command'][-1],str(cwd/'input.txt'))

    def test_existing_executable_retains_original_path_and_image(self):
        executable=self.work/'control.exe'
        executable.write_bytes(b'unchanged image')
        expected=self.work/'expected.txt'
        expected.write_text('42\n')
        report_file=self.work/'report.json'
        calls=0
        def run(command,**kwargs):
            nonlocal calls
            self.assertEqual(command,[executable.resolve()])
            calls+=1
            return subprocess.CompletedProcess(command,0,'' if calls==1 else '42\n','')
        with patch('failure_artifacts.subprocess.run',side_effect=run), contextlib.redirect_stdout(io.StringIO()):
            status=stress_native_output.main(['--executable',str(executable),'--expected-file',str(expected),
                                             '--diagnostic-runs','2','--report',str(report_file)])
        self.assertEqual(status,1)
        self.assertEqual(calls,3)
        folder,report=self.retained()
        self.assertEqual((folder/'control.exe').read_bytes(),b'unchanged image')
        self.assertEqual(report['commands'][0]['retained_command'],[str(folder/'control.exe')])
        self.assertEqual(report['commands'][0]['command'],[str(executable.resolve())])

    def test_diagnostic_timeout_keeps_native_failure_classification(self):
        executable=self.work/'control.exe'
        executable.write_bytes(b'original image')
        expected=self.work/'expected.txt'
        expected.write_text('42\n')
        report_file=self.work/'report.json'
        responses=[subprocess.CompletedProcess([],0,'',''),
                   subprocess.TimeoutExpired([str(executable)],.1,output=b'partial')]
        with patch('failure_artifacts.subprocess.run',side_effect=responses), contextlib.redirect_stdout(io.StringIO()):
            status=stress_native_output.main(['--executable',str(executable),'--expected-file',str(expected),
                                             '--report',str(report_file)])
        self.assertEqual(status,1)
        row=json.loads(report_file.read_text())['cases'][0]
        self.assertEqual(row['status'],'native_failure')
        self.assertEqual(row['runs'][0]['stdout'],'')
        self.assertIn('timed out',row['diagnostic_error'])
        folder,report=self.retained()
        self.assertEqual(report['commands'][0]['returncode'],0)
        self.assertIn('TimeoutExpired',report['commands'][1]['exception'])

    def test_stress_diagnostics_do_not_replace_original_failure(self):
        source=self.work/'source.f90'
        source.write_text('print *,42\nend\n')
        expected=self.work/'expected.txt'
        expected.write_text('42\n')
        report_file=self.work/'report.json'
        executions=0
        compiled=0
        def run(command,**kwargs):
            nonlocal executions,compiled
            if 'emit-obj' in command:
                compiled+=1
                Path(command[-1]).write_bytes(b'original object')
                return subprocess.CompletedProcess(command,0,'IR','')
            if 'link' in command:
                self.assertEqual(Path(command[2]).read_bytes(),b'original object')
                Path(command[-1]).write_bytes(b'relinked' if 'relinked' in str(command[-1]) else b'original image')
                return subprocess.CompletedProcess(command,0,'','')
            executions+=1
            return subprocess.CompletedProcess(command,0,'' if executions==1 else '42\n','')
        args=[str(source),'--compiler',sys.executable,'--iterations','3','--runs-per-build','2',
              '--diagnostic-runs','2','--expected-file',str(expected),'--report',str(report_file)]
        with patch('failure_artifacts.subprocess.run',side_effect=run), contextlib.redirect_stdout(io.StringIO()):
            status=stress_native_output.main(args)
        self.assertEqual(status,1)
        self.assertEqual(compiled,1)
        report=json.loads(report_file.read_text())
        self.assertEqual(len(report['cases']),1)
        row=report['cases'][0]
        self.assertEqual(row['status'],'native_failure')
        self.assertEqual(row['runs'][0]['stdout'],'')
        self.assertEqual(row['diagnosis']['same_executable_matches'],[True,True])
        self.assertEqual(row['diagnosis']['relinked_executable_matches'],[True,True])
        folder,details=self.retained()
        self.assertEqual((folder/('original.exe' if os.name=='nt' else 'original')).read_bytes(),b'original image')
        self.assertTrue(details['failures'])


if __name__=='__main__':
    unittest.main()
