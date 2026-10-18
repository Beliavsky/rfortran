import contextlib
import io
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest import mock

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))
import run_batch


class BatchTests(unittest.TestCase):
    def setUp(self):
        parent = ROOT.parent / "target"
        parent.mkdir(exist_ok=True)
        self.temp = tempfile.TemporaryDirectory(prefix="batch-runner-test-", dir=parent)
        self.addCleanup(self.temp.cleanup)
        self.work = Path(self.temp.name)

    def source(self, name):
        path = self.work / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text("implicit none\nprint *,1\nend\n", encoding="utf-8")
        return path.resolve()

    def test_globs_sorted_deduplicated_and_missing_reported(self):
        a, b = self.source("a.f90"), self.source("sub/b.f90")
        files, errors = run_batch.collect_inputs([str(self.work / "**/*.f90"), str(a), str(self.work / "missing*.f90")])
        self.assertEqual(files, [a, b])
        self.assertEqual(len(errors), 1)
        self.assertEqual(errors[0]["status"], "input_failed")

    def test_identical_basenames_have_distinct_stable_outputs(self):
        a, b = self.source("a/main.f90"), self.source("b/main.f90")
        self.assertNotEqual(run_batch.artifact_dir(self.work,a),run_batch.artifact_dir(self.work,b))
        self.assertEqual(run_batch.artifact_dir(self.work,a),run_batch.artifact_dir(self.work,a))

    def test_timeout_and_missing_executable_become_stage_results(self):
        error = subprocess.TimeoutExpired(["tool"],1,output=b"partial",stderr=b"diagnostic")
        with mock.patch.object(run_batch.subprocess,"run",side_effect=error):
            result = run_batch.run_phase("run",["tool"],self.work,1)
        self.assertEqual(result["status"],"timeout")
        self.assertEqual(result["stdout"],"partial")
        self.assertEqual(result["stderr"],"diagnostic")
        with mock.patch.object(run_batch.subprocess,"run",side_effect=FileNotFoundError("tool missing")):
            result = run_batch.run_phase("compile",["tool"],self.work,1)
        self.assertEqual(result["status"],"failed")
        self.assertIn("tool missing",result["diagnostic"])

    def invoke(self, *args):
        with contextlib.redirect_stdout(io.StringIO()):
            return run_batch.main([*map(str,args),"--out-dir",str(self.work / "output")])

    def phase(self, name, command, cwd, timeout):
        return dict(stage=name,status="ok",returncode=0,stdout="hello\n" if name=="run" else "",stderr="",seconds=0)

    def test_translation_mode_does_not_compile_or_run(self):
        source = self.source("a.f90")
        with mock.patch.object(run_batch,"run_phase",side_effect=self.phase) as runner:
            self.assertEqual(self.invoke(source),0)
        self.assertEqual(runner.call_count,1)
        report = json.loads((self.work / "output/report.json").read_text())
        self.assertEqual(report["summary"],{"translated":1})

    def test_compile_failure_stops_that_file_and_continues_batch(self):
        a, b = self.source("a.f90"), self.source("b.f90")
        calls = []
        def phase(name,command,cwd,timeout):
            calls.append(name)
            result = self.phase(name,command,cwd,timeout)
            if len(calls)==2:
                result.update(status="failed",returncode=1,stderr="bad Rust")
            return result
        with mock.patch.object(run_batch,"run_phase",side_effect=phase):
            self.assertEqual(self.invoke(a,b,"--run"),1)
        self.assertEqual(calls,["translate","compile","translate","compile","run"])
        report = json.loads((self.work / "output/report.json").read_text())
        self.assertEqual(report["summary"],{"compile_failed":1,"passed":1})
        self.assertEqual(report["results"][1]["stages"][-1]["stdout"],"hello\n")

    def test_real_subprocess_captures_failure_and_closes_stdin(self):
        result = run_batch.run_phase("run",[sys.executable,"-c","import sys;print(sys.stdin.read());print('failure',file=sys.stderr);sys.exit(7)"],self.work,10)
        self.assertEqual(result["returncode"],7)
        self.assertEqual(result["status"],"failed")
        self.assertIn("failure",result["stderr"])

    def test_no_matches_produces_failure_report(self):
        self.assertEqual(self.invoke(self.work / "absent*.f90"),1)
        report = json.loads((self.work / "output/report.json").read_text())
        self.assertEqual(report["summary"],{"input_failed":1})


if __name__ == "__main__":
    unittest.main()
