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
        self.assertNotEqual(run_batch.artifact_dir(self.work,a,[a]),run_batch.artifact_dir(self.work,a,[b]))

    def test_common_modules_precede_each_driver_and_are_excluded_from_glob(self):
        module,source = self.source("mod.f90"),self.source("main.f90")
        with mock.patch.object(run_batch,"run_phase",side_effect=self.phase) as runner:
            self.assertEqual(self.invoke(self.work / "*.f90","--module",module),0)
        command = runner.call_args.args[1]
        self.assertEqual(command[2:4],[module,source])
        report = json.loads((self.work / "output/report.json").read_text())
        self.assertEqual(report["modules"],[str(module)])
        self.assertEqual(report["summary"],{"translated":1})

    def test_multiple_and_repeated_module_options_preserve_order(self):
        first,second,third = [self.source(name) for name in ("m1.f90","m2.f90","m3.f90")]
        source = self.source("driver.f90")
        for options in (("--module",first,second,third),
                        ("--module",first,"--module",second,"--module",third),
                        ("--module",first,second,"--module",third)):
            with self.subTest(options=options):
                with mock.patch.object(run_batch,"run_phase",side_effect=self.phase) as runner:
                    self.assertEqual(self.invoke(source,*options,"--run"),0)
                command = runner.call_args_list[0].args[1]
                self.assertEqual(command[2:6],[first,second,third,source])
                report = json.loads((self.work / "output/report.json").read_text())
                self.assertEqual(report["modules"],list(map(str,(first,second,third))))

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

    def test_limit_applies_after_deduplication_and_module_exclusion(self):
        module = self.source("00_module.f90")
        drivers = [self.source(name) for name in ("a.f90","b.f90","c.f90")]
        for limit in (2,99):
            with self.subTest(limit=limit):
                with mock.patch.object(run_batch,"run_phase",side_effect=self.phase) as runner:
                    self.assertEqual(self.invoke(self.work / "*.f90",drivers[0],"--module",module,"--limit",limit),0)
                selected = drivers[:limit]
                self.assertEqual([call.args[1][3] for call in runner.call_args_list],selected)
                report = json.loads((self.work / "output/report.json").read_text())
                self.assertEqual(report["limit"],limit)
                self.assertEqual(report["drivers_matched"],3)
                self.assertEqual(report["drivers_selected"],len(selected))
                self.assertEqual(report["drivers_skipped"],list(map(str,drivers[len(selected):])))

    def test_failures_count_toward_limit_and_do_not_stop_selected_drivers(self):
        drivers = [self.source(name) for name in ("a.f90","b.f90","c.f90")]
        def phase(name,command,cwd,timeout):
            result = self.phase(name,command,cwd,timeout)
            if command[2]==drivers[0]:
                result.update(status="failed",returncode=1,stderr="unsupported feature")
            return result
        with mock.patch.object(run_batch,"run_phase",side_effect=phase) as runner:
            self.assertEqual(self.invoke(*drivers,"--limit",2),1)
        self.assertEqual(runner.call_count,2)
        report = json.loads((self.work / "output/report.json").read_text())
        self.assertEqual(report["summary"],{"translate_failed":1,"translated":1})
        self.assertEqual(report["drivers_skipped"],[str(drivers[2])])

    def test_limit_requires_a_positive_integer(self):
        source = self.source("main.f90")
        for value in ("0","-1","1.5","abc"):
            with self.subTest(value=value),contextlib.redirect_stderr(io.StringIO()),self.assertRaises(SystemExit) as caught:
                self.invoke(source,"--limit",value)
            self.assertEqual(caught.exception.code,2)


if __name__ == "__main__":
    unittest.main()
