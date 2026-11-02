"""Unix C-linker regression for the standalone no_std formatting runtime.

Run with python tests/test_native_runtime_link.py on Linux or macOS.
No rfort rebuild is required; Rust and the platform C toolchain are required.
"""
import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]

HARNESS = r'''
#include <stddef.h>
#include <stdint.h>
extern void *rfort_format_begin(const unsigned char *, size_t, int32_t);
extern int32_t rfort_format_push(void *, uint64_t, uint64_t, uint64_t);
extern int32_t rfort_format_end(void *);
extern _Noreturn void rust_eh_personality(int32_t, int32_t, uint64_t, void *, void *);
int main(int argc, char **argv) {
    (void)argv;
    if (argc > 1) rust_eh_personality(1, 0, 0, NULL, NULL);
    void *list = rfort_format_begin((const unsigned char *)"(i0)", 4, 1);
    if (rfort_format_push(list, 1, 42, 4)) return 1;
    return rfort_format_end(list);
}
'''


@unittest.skipUnless(sys.platform in ('linux', 'darwin'), 'requires a native Unix C linker')
class NativeRuntimeLink(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        import resource
        resource.setrlimit(resource.RLIMIT_CORE, (0, 0))
        cls.temporary = tempfile.TemporaryDirectory(prefix='rfort native runtime ')
        cls.addClassCleanup(cls.temporary.cleanup)
        cls.work = Path(cls.temporary.name)
        library = cls.work / 'librfort_format.a'
        cls.command([os.environ.get('RUSTC', 'rustc'), '--crate-name', 'rfort_format',
                     '--crate-type', 'staticlib', '--edition=2021', '-C', 'opt-level=2',
                     '-C', 'panic=abort', '--cfg', 'native_runtime',
                     str(ROOT / 'src/native_format.rs'), '-o', str(library)], cwd=ROOT)
        harness = cls.work / 'harness.c'
        harness.write_text(HARNESS, encoding='utf-8')
        cls.executable = cls.work / 'runtime-check'
        compiler = os.environ.get('RFORT_CC') or os.environ.get('CC')
        command = ([compiler] if compiler else
                   ['xcrun', '--sdk', 'macosx', 'clang'] if sys.platform == 'darwin' else ['cc'])
        # Do not rely on garbage collection of unused sections to discard the
        # core library's references to rust_eh_personality.
        command += [str(harness), str(library), '-o', str(cls.executable)]
        if sys.platform == 'linux':
            command.append('-lm')
        cls.command(command, cwd=cls.work)

    @staticmethod
    def command(command, **kwargs):
        result = subprocess.run(command, capture_output=True, text=True, timeout=120, **kwargs)
        if result.returncode:
            raise AssertionError(f'{command}\n{result.stdout}\n{result.stderr}')
        return result

    def test_plain_c_linker_runs_formatting_runtime(self):
        result = self.command([self.executable], cwd=self.work)
        self.assertEqual(result.stdout.strip(), '42')

    def test_unexpected_unwind_aborts(self):
        result = subprocess.run([self.executable, 'unwind'], cwd=self.work,
                                capture_output=True, text=True, timeout=15)
        self.assertEqual(result.returncode, -signal.SIGABRT)


if __name__ == '__main__':
    unittest.main()
