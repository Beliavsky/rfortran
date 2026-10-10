use crate::cli::Cli;
use anyhow::{bail, Context, Result};
use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};

#[cfg(all(windows, target_pointer_width = "64"))]
const MSVCRT_LIB: &[u8] = include_bytes!("libs/x64/msvcrt.lib");
#[cfg(all(windows, target_pointer_width = "64"))]
const KERNEL32_LIB: &[u8] = include_bytes!("libs/x64/kernel32.lib");
#[cfg(all(windows, target_pointer_width = "64"))]
const LEGACY_STDIO_LIB: &[u8] = include_bytes!("libs/x64/legacy_stdio_definitions.lib");

#[cfg(all(windows, target_pointer_width = "32"))]
const MSVCRT_LIB: &[u8] = include_bytes!("libs/x86/msvcrt.lib");
#[cfg(all(windows, target_pointer_width = "32"))]
const KERNEL32_LIB: &[u8] = include_bytes!("libs/x86/kernel32.lib");
#[cfg(all(windows, target_pointer_width = "32"))]
const LEGACY_STDIO_LIB: &[u8] = include_bytes!("libs/x86/legacy_stdio_definitions.lib");

struct WorkDir(PathBuf);
impl WorkDir {
    fn new() -> Result<Self> {
        let path = env::temp_dir().join(format!("rfort-link-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&path)?;
        Ok(Self(path))
    }
}
impl Drop for WorkDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn run(command: &mut Command, cli: &Cli) -> Result<()> {
    if !cli.quiet {
        eprintln!("[link] {command:?}");
    }
    let output = command
        .output()
        .with_context(|| format!("could not execute {command:?}"))?;
    if !output.status.success() {
        bail!(
            "tool failed ({})\n{}{}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    // Keep tool warnings visible even for otherwise silent successful builds.
    if !output.stdout.is_empty() {
        eprint!("{}", String::from_utf8_lossy(&output.stdout));
    }
    if !output.stderr.is_empty() {
        eprint!("{}", String::from_utf8_lossy(&output.stderr));
    }
    Ok(())
}

#[cfg(windows)]
fn latest_directory(root: &Path, suffix: &Path) -> Option<PathBuf> {
    let mut candidates: Vec<_> = fs::read_dir(root)
        .ok()?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.join(suffix).is_dir())
        .collect();
    candidates.sort_by_key(|p| {
        p.file_name()
            .unwrap()
            .to_string_lossy()
            .split('.')
            .map(|v| v.parse::<u32>().unwrap_or(0))
            .collect::<Vec<_>>()
    });
    candidates.pop().map(|p| p.join(suffix))
}

#[cfg(windows)]
fn windows_libraries() -> Result<Vec<PathBuf>> {
    let mut paths = Vec::new();
    if let Some(value) = env::var_os("RFORT_LIB_PATH") {
        for path in env::split_paths(&value) {
            if !path.is_dir() {
                bail!(
                    "RFORT_LIB_PATH directory does not exist: {}",
                    path.display()
                );
            }
            paths.push(path);
        }
    }
    if let Some(value) = env::var_os("LIB") {
        paths.extend(env::split_paths(&value));
    }
    let arch = if cfg!(target_arch = "aarch64") {
        "arm64"
    } else if cfg!(target_pointer_width = "64") {
        "x64"
    } else {
        "x86"
    };
    let suffix = PathBuf::from("lib").join(arch);
    if let Some(root) = env::var_os("VCToolsInstallDir") {
        let path = PathBuf::from(root).join(&suffix);
        if path.is_dir() {
            paths.push(path);
        }
    }
    let program_files = env::var_os("ProgramFiles(x86)").or_else(|| env::var_os("ProgramFiles"));
    if let Some(root) = &program_files {
        let vswhere = PathBuf::from(root).join("Microsoft Visual Studio/Installer/vswhere.exe");
        if let Ok(output) = Command::new(vswhere)
            .args([
                "-latest",
                "-products",
                "*",
                "-requires",
                "Microsoft.VisualStudio.Component.VC.Tools.x86.x64",
                "-property",
                "installationPath",
                "-utf8",
            ])
            .output()
        {
            if output.status.success() {
                let install = String::from_utf8_lossy(&output.stdout).trim().to_owned();
                if !install.is_empty() {
                    if let Some(path) =
                        latest_directory(&PathBuf::from(install).join("VC/Tools/MSVC"), &suffix)
                    {
                        paths.push(path);
                    }
                }
            }
        }
    }
    let sdk = env::var_os("WindowsSdkDir")
        .map(PathBuf::from)
        .or_else(|| program_files.map(|p| PathBuf::from(p).join("Windows Kits/10")));
    if let Some(root) = sdk {
        for component in ["ucrt", "um"] {
            if let Some(path) =
                latest_directory(&root.join("Lib"), &PathBuf::from(component).join(arch))
            {
                paths.push(path);
            }
        }
    }
    Ok(paths)
}

pub fn link(objects: &[PathBuf], out: &Path, cli: &Cli) -> Result<()> {
    if cli.lto {
        bail!("--lto is not supported for Cranelift native objects");
    }
    let work = WorkDir::new()?;
    let format_runtime = work.0.join("rfort_format.lib");
    fs::write(&format_runtime, include_bytes!(concat!(env!("OUT_DIR"), "/rfort_format.lib")))?;
    #[cfg(target_os = "windows")]
    {
        let tool = if let Some(path) = env::var_os("RFORT_LINKER") {
            PathBuf::from(path)
        } else {
            which::which("ld.lld.exe").or_else(|_| which::which("lld-link.exe"))
                .context("No LLVM linker found; install LLVM and put ld.lld.exe or lld-link.exe on PATH, or set RFORT_LINKER")?
        };
        let mut command = Command::new(&tool);
        if tool
            .file_stem()
            .is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case("ld.lld"))
        {
            command.args(["-flavor", "link"]);
        }
        command.args(["/NOLOGO", "/SUBSYSTEM:CONSOLE"]);
        let mut output_arg = std::ffi::OsString::from("/OUT:");
        output_arg.push(out);
        command.arg(output_arg).args(objects).arg(&format_runtime);
        for (name, bytes) in [
            ("msvcrt.lib", MSVCRT_LIB),
            ("kernel32.lib", KERNEL32_LIB),
            ("legacy_stdio_definitions.lib", LEGACY_STDIO_LIB),
        ] {
            fs::write(work.0.join(name), bytes)?;
        }
        let paths = std::iter::once(work.0.clone()).chain(windows_libraries()?);
        for path in paths {
            let mut arg = std::ffi::OsString::from("/LIBPATH:");
            arg.push(path);
            command.arg(arg);
        }
        command.args(["msvcrt.lib", "legacy_stdio_definitions.lib", "kernel32.lib"]);
        run(&mut command, cli).context("Windows linking failed; install Visual Studio C++ Build Tools and the Windows SDK, or set RFORT_LIB_PATH to their library directories")?;
    }
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        let compiler = env::var_os("RFORT_CC").or_else(|| env::var_os("CC"));
        let make_command = || {
            if let Some(tool) = &compiler {
                Command::new(tool)
            } else if cfg!(target_os = "macos") {
                let mut c = Command::new("xcrun");
                c.args(["--sdk", "macosx", "clang"]);
                c
            } else {
                Command::new("cc")
            }
        };
        let source = work.0.join("runtime.c");
        let object = work.0.join("runtime.o");
        fs::write(&source, include_str!("libs/portable_runtime.c"))?;
        run(
            make_command()
                .args(["-c", "-O2"])
                .arg(&source)
                .arg("-o")
                .arg(&object),
            cli,
        )?;
        let mut command = make_command();
        command.args(objects).arg(&object).arg(&format_runtime).arg("-o").arg(out);
        #[cfg(target_os = "linux")]
        command.args(["-no-pie", "-lm"]);
        run(&mut command, cli)?;
    }
    #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
    bail!("Native linking is supported only on Windows, Linux, and macOS");
    Ok(())
}
