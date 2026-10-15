use std::path::PathBuf;
use structopt::StructOpt;

#[derive(Debug, StructOpt, Clone)]
#[structopt(
    name = "rfort",
    about = "Fortran compiler written in Rust"
)]
pub struct Cli {
    pub inputs: Vec<PathBuf>,

    #[structopt(
        short = "o",
        long,
        help = "Output object/executable path (defaults derived from last source)"
    )]
    pub out: Option<PathBuf>,

    #[structopt(long = "Wall", help = "Enable all warnings")]
    pub wall: bool,

    #[structopt(long = "Werror", help = "Treat warnings as errors")]
    pub werror: bool,

    #[structopt(long = "warning", number_of_values = 1, help = "Control a warning (e.g. optional-argument or no-unused-type)")]
    pub warnings: Vec<String>,

    #[structopt(
        long = "lto",
        help = "Enable Link Time Optimization (LTO) for better performance"
    )]
    pub lto: bool,

    #[structopt(
        short = "O",
        long = "opt-level",
        value_name = "LEVEL",
        default_value = "2",
        possible_values = &["0","1","2","3","s","z","S","Z"],
        help = "\
    Optimization level:
    0 = none
    1 = speed (quick)
    2 = speed (default)
    3 = speed (verifier off)
    s = speed and size
    z = smallest size
    S = speed and size (aggressive)
    Z = smallest size (aggressive)"
    )]
    pub opt_level: String,

    #[structopt(
        long = "quiet",
        short = "q",
        help = "Suppress build progress (warnings and errors remain visible)"
    )]
    pub quiet: bool,

    #[structopt(long, help = "Show compilation and linker commands")]
    pub verbose: bool,

    #[structopt(short = "c", long = "compile-only", help = "Emit an object without linking")]
    pub compile_only: bool,

    #[structopt(short = "I", long = "module-path", number_of_values = 1, help = "Search directory for rfort module interfaces (repeatable)")]
    pub module_paths: Vec<PathBuf>,

    #[structopt(short = "J", long = "module-dir", help = "Write module interfaces here and also search this directory")]
    pub module_dir: Option<PathBuf>,

    #[structopt(long = "debug-ir", help = "Show IR before and after optimizations")]
    pub debug_ir: bool,

    #[structopt(
        long = "target",
        help = "Target triple for code generation (default: host target)"
    )]
    #[allow(dead_code)]
    // not used now
    pub target: Option<String>,

    #[structopt(short = "S", long = "emit-asm", help = "Print generated assembly")]
    pub emit_asm: bool,

    #[structopt(long = "help", short = "h", help = "Show this help message")]
    pub help: bool,

    #[structopt(subcommand)]
    pub cmd: Option<Command>,

    #[structopt(long = "version", short = "v", help = "Show version information")]
    pub version: bool,

    #[structopt(
        long = "run",
        help = "Run after building (only applies if no subcommand)"
    )]
    pub run: bool,
}

#[derive(Debug, StructOpt, Clone)]
pub enum Command {
    Lex {
        input: PathBuf,
        out: Option<PathBuf>,
    },
    Parse {
        input: PathBuf,
        out: Option<PathBuf>,
    },
    Check {
        input: PathBuf,
    },
    Build {
        #[structopt(required = true)]
        inputs: Vec<PathBuf>,
        #[structopt(short = "o", long)]
        out: Option<PathBuf>,
        #[structopt(long)]
        run: bool,
    },
    Link {
        inputs: Vec<PathBuf>,
        #[structopt(short = "o", long)]
        out: Option<PathBuf>,
    },
    EmitObj {
        input: PathBuf,
        out: PathBuf,
    },
    Run {
        input: PathBuf,
    },
    Help,
}

impl Cli {
    pub fn from_command_line() -> Self {
        let mut args = Vec::<std::ffi::OsString>::new();
        for arg in std::env::args_os() {
            let text = arg.to_string_lossy();
            match text.as_ref() {
                "-Wall" | "--wall" => args.push("--Wall".into()),
                "-Werror" | "--werror" => args.push("--Werror".into()),
                flag if flag.starts_with("-W") => {
                    args.push("--warning".into());
                    args.push(flag[2..].into());
                }
                _ => args.push(arg),
            }
        }
        Self::from_iter(args)
    }

    pub fn normalize(mut self) -> Self {
        self.quiet = self.quiet || !self.verbose;
        if self.cmd.is_none() && !self.inputs.is_empty() {
            self.cmd = Some(Command::Build {
                inputs: self.inputs.clone(), out: self.out.clone(), run: self.run,
            });
        }
        if let Some(Command::Lex {
            ref input,
            out: ref output,
        }) = self.cmd
        {
            if output.is_none() {
                let out = self.out.clone();
                self.cmd = Some(Command::Lex {
                    input: input.to_path_buf(),
                    out,
                });
            }
        }
        if let Some(Command::Parse {
            ref input,
            out: ref output,
        }) = self.cmd
        {
            if output.is_none() {
                let out = self.out.clone();
                self.cmd = Some(Command::Parse {
                    input: input.to_path_buf(),
                    out,
                });
            }
        }
        self
    }
}
