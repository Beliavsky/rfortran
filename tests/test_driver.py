"""Portable native driver checks. Build first: cargo build --bin rfort."""
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

from test_record_read import ROOT, COMPILER

class NativeDriver(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory(prefix="rfort driver (A&B)! ")
        self.addCleanup(self.tmp.cleanup)
        self.directory = Path(self.tmp.name)

    def source(self, name="main.f90", text="print *, 42\nend\n"):
        path = self.directory / name
        path.write_text(text, encoding="utf-8-sig")
        return path

    def compile(self, *args, env=None, compiler=COMPILER):
        return subprocess.run([compiler, *map(str, args)], cwd=self.directory,
                              capture_output=True, text=True, timeout=60, env=env)

    def success(self, *args, **kwargs):
        result = self.compile(*args, **kwargs)
        self.assertEqual(result.returncode, 0, result.stdout+result.stderr)
        self.assertEqual(result.stdout+result.stderr, "")
        return result

    def execute(self, exe, expected="42"):
        result = subprocess.run([exe], capture_output=True, text=True, timeout=15)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stderr, "")
        self.assertEqual(result.stdout.strip(), expected)

    def exe(self, name="main"):
        return self.directory / (name+".exe" if os.name=="nt" else name)

    def test_default_and_explicit_outputs(self):
        source = self.source()
        self.success(source)
        self.execute(self.exe())
        output = self.exe("custom program")
        self.success("-o",output,source)
        self.execute(output)
        self.success("build",source,"-o",output)
        self.execute(output)

    def test_sources_named_like_subcommands(self):
        # Relative names, since a path separator would hide the clap check.
        for name in ("run.f90", "help.f90", "parse.f90", "hello.f90"):
            with self.subTest(name=name):
                self.source(name)
                self.success(name, "-o", "named.exe")
                self.execute(self.directory/"named.exe")
                self.success("-O0", name, "-o", "named.exe")
                self.execute(self.directory/"named.exe")
        for name in ("m1", "m2", "main"):
            self.source({"m1": "link", "m2": "lex", "main": "run"}[name]+".f90",
                        (ROOT/"tests/multifile"/(name+".f90")).read_text())
        self.success("link.f90", "lex.f90", "run.f90", "-o", "named.exe")
        self.execute(self.directory/"named.exe")
        # Subcommands, and suggestions for mistyped ones, are unchanged.
        result = self.compile("run", "run.f90")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout.strip(), "42")
        result = self.compile("buld", "run.f90")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Did you mean 'build'", result.stderr)

    def test_module_chain_and_repeat_build(self):
        sources=[]
        for name in ("m1", "m2", "main"):
            sources.append(self.source(name+" source.f90", (ROOT/"tests/multifile"/(name+".f90")).read_text()+"\n! trailing comment"))
        self.success(*sources)
        self.execute(self.exe("main source"))
        self.success(*sources)
        self.execute(self.exe("main source"))
        combined=sources[-1].with_suffix(".f90c-sources.f90")
        text=combined.read_text()
        self.assertNotIn("\ufeff",text)
        self.assertLess(text.index("module m1"),text.index("module m2"))
        self.assertLess(text.index("module m2"),text.index("program main"))
        output=self.exe("custom modules")
        self.success("build",*sources,"-o",output)
        self.execute(output)

    def test_object_only_and_object_linking(self):
        source=self.source()
        obj=self.directory/"separate object.o"
        self.success("-c",source,"-o",obj)
        self.assertTrue(obj.exists())
        self.assertFalse(self.exe().exists())
        self.success("link",obj,"-o",self.exe())
        self.execute(self.exe())
        self.success(obj,"-o",self.exe("from object"))
        self.execute(self.exe("from object"))
        library=self.source("library.f90","module library\ninteger,parameter::n=3\nend module library\n")
        self.success(library)
        self.assertTrue(library.with_suffix(".obj" if os.name=="nt" else ".o").exists())
        self.assertFalse(self.exe("library").exists())

    def test_run_and_verbose(self):
        source=self.source()
        result=self.compile("--run",source)
        self.assertEqual(result.returncode,0,result.stderr)
        self.assertEqual(result.stdout.strip(),"42")
        self.assertEqual(result.stderr,"")
        result=self.compile("--verbose",source)
        self.assertEqual(result.returncode,0,result.stderr)
        self.assertIn("[link]",result.stderr)
        self.success("--quiet","--verbose",source)
        source=self.source(text="error stop 'intentional'\nend\n")
        result=self.compile("--run",source)
        self.assertNotEqual(result.returncode,0)
        self.assertIn("intentional",result.stderr)

    def test_failure_preserves_inputs_and_previous_executable(self):
        source=self.source()
        original=source.read_bytes()
        self.exe().write_bytes(b"previous executable")
        result=self.compile(source,self.directory/"missing.f90")
        self.assertNotEqual(result.returncode,0)
        self.assertIn("Source file not found",result.stderr)
        self.assertEqual(self.exe().read_bytes(),b"previous executable")
        for output in [source,source.with_suffix(".obj" if os.name=="nt" else ".o")]:
            result=self.compile(source,"-o",output)
            self.assertNotEqual(result.returncode,0)
        self.assertEqual(source.read_bytes(),original)
        library=self.source("library.f90","module library\nend module library\n")
        combined=source.with_suffix(".f90c-sources.f90")
        combined.write_text("! handwritten\n")
        result=self.compile(library,source)
        self.assertNotEqual(result.returncode,0)
        self.assertIn("Refusing to overwrite",result.stderr)
        self.assertEqual(combined.read_text(),"! handwritten\n")

    def test_copied_executable_needs_no_repository_or_powershell(self):
        tool=self.directory/("copied rfort.exe" if os.name=="nt" else "copied rfort")
        shutil.copy2(COMPILER,tool)
        environment=os.environ.copy()
        if os.name=="nt":
            linker=shutil.which("ld.lld.exe")
            self.assertIsNotNone(linker)
            environment["RFORT_LINKER"]=linker
            environment["PATH"]=str(Path(linker).parent)
        self.success(self.source(),compiler=tool,env=environment)
        self.execute(self.exe())

    def test_invalid_tool_override(self):
        environment=os.environ.copy()
        environment["RFORT_LINKER" if os.name=="nt" else "RFORT_CC"]=str(self.directory/"missing tool")
        result=self.compile(self.source(),env=environment)
        self.assertNotEqual(result.returncode,0)
        self.assertIn("could not execute",result.stderr)

    def test_legacy_generated_source_is_refreshed(self):
        library=self.source("library.f90","module library\nend module library\n")
        source=self.source()
        source.with_suffix(".f90c-sources.f90").write_text("! Generated by f90c.bat; do not edit.\nstale\n")
        self.success(library,source)
        self.execute(self.exe())

if __name__=="__main__":
    unittest.main(verbosity=2)
