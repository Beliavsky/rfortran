"""Nonadvancing stdout WRITE, record continuation, and prompt flushing."""
from pathlib import Path
import queue
import shutil
import subprocess
import tempfile
import threading
import unittest
from test_record_read import ROOT, COMPILER, build_commands


class NonadvancingOutput(unittest.TestCase):
    def test_records_and_reference(self):
        source = ROOT / "tests/85_nonadvancing_output.f90"
        expected = "left:  7\nempty-end\nloop: 1 2 3\nmixed: done\nslash:\nnext-end\nfinal"
        with tempfile.TemporaryDirectory(prefix="nonadvancing-") as tmp:
            directory = Path(tmp)
            commands = build_commands(source, directory)
            if shutil.which("gfortran"):
                exe = directory / "reference.exe"
                result = subprocess.run(["gfortran", source, "-o", exe], cwd=directory, capture_output=True, text=True, timeout=60)
                self.assertEqual(result.returncode, 0, result.stderr)
                commands.append([exe])
            for command in commands:
                with self.subTest(command=command):
                    result = subprocess.run(command, capture_output=True, text=True, timeout=15)
                    self.assertEqual(result.returncode, 0, result.stderr)
                    self.assertEqual(result.stdout, expected)
                    self.assertEqual(result.stderr, "")

    def test_prompt_is_flushed_before_input(self):
        with tempfile.TemporaryDirectory(prefix="prompt-") as tmp:
            directory = Path(tmp)
            source = directory / "main.f90"
            source.write_text("""program main
character(1) :: answer
write (*,'(a)',advance='no') 'prompt:'
read (*,'(a)') answer
write (*,'(a)') answer
end program main
""")
            for command in build_commands(source, directory):
                with self.subTest(command=command):
                    process = subprocess.Popen(command, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
                    received = queue.Queue()
                    reader = threading.Thread(target=lambda: received.put(process.stdout.read(7)), daemon=True)
                    reader.start()
                    try:
                        # The child cannot finish READ until we see the flushed prompt.
                        self.assertEqual(received.get(timeout=10), b"prompt:")
                        stdout, stderr = process.communicate(b"x\n", timeout=10)
                        self.assertEqual(process.returncode, 0, stderr)
                        self.assertEqual(stdout.replace(b"\r\n", b"\n"), b"x\n")
                        self.assertEqual(stderr, b"")
                    finally:
                        if process.poll() is None:
                            process.kill()
                        process.communicate()
                        reader.join(timeout=5)

    def test_stop_preserves_nonadvancing_output(self):
        with tempfile.TemporaryDirectory(prefix="stop-record-") as tmp:
            directory = Path(tmp)
            source = directory / "main.f90"
            source.write_text("program main\nwrite (*,'(a)',advance='no') 'stop'\nstop\nend program main\n")
            for command in build_commands(source, directory):
                result = subprocess.run(command, capture_output=True, text=True, timeout=15)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(result.stdout, "stop")

    def test_control_diagnostics(self):
        cases = [
            ("", "write (*,*,advance='no') 1", "explicit format"),
            ("", "write (*,*,advance='yes') 1", "explicit format"),
            ("", "write (*,'(i1)',advance='maybe') 1", "'yes' or 'no'"),
            ("", "write (*,'(i1)',advance=1) 1", "scalar CHARACTER"),
            ("", "write (*,'(i1)',advance='no',advance='yes') 1", "duplicate WRITE control"),
            ("", "write (*,'(i1)',unknown='no') 1", "unsupported WRITE control"),
        ]
        with tempfile.TemporaryDirectory(prefix="bad-advance-") as tmp:
            source = Path(tmp) / "main.f90"
            for declarations, statement, message in cases:
                with self.subTest(statement=statement):
                    source.write_text("program main\n"+declarations+"\n"+statement+"\nend program main\n")
                    result = subprocess.run([COMPILER, "check", source], capture_output=True, text=True, timeout=15)
                    self.assertNotEqual(result.returncode, 0)
                    self.assertIn(message, result.stderr)


if __name__ == "__main__":
    unittest.main(verbosity=2)
