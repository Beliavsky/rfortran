use std::path::PathBuf;
use structopt::StructOpt;

#[derive(Debug, StructOpt, Clone)]
#[structopt(
    name = "rfort",
    about = "Fortran compiler written in Rust"
)]
pub struct Cli {
    #[structopt(skip)]
    pub run_args: Vec<std::ffi::OsString>,
    pub inputs: Vec<PathBuf>,

    #[structopt(long = "std", global = true, possible_values = &["f90","f95","f2003","f2008","f2018","f2023"],
                help = "Check implemented features against a Fortran revision (default: permissive)")]
    pub standard: Option<crate::standards::Standard>,

    #[structopt(
        short = "o",
        long,
        help = "Output object/executable path (defaults derived from last source)"
    )]
    pub out: Option<PathBuf>,

    #[structopt(long = "Wall", help = "Enable common warnings (some advice remains opt-in)")]
    pub wall: bool,

    #[structopt(long = "Werror", help = "Treat warnings as errors")]
    pub werror: bool,

    #[structopt(long = "ffixed-form", global = true, help = "Read sources as fixed form (default for .f, .for, .ftn and .f77)")]
    pub fixed_form: bool,

    #[structopt(long = "ffree-form", global = true, conflicts_with = "fixed_form",
                help = "Read sources as free form, whatever their extension")]
    pub free_form: bool,

    #[structopt(long = "ffixed-line-length", global = true, value_name = "N|none",
                help = "Last column of a fixed-form line (default 72)")]
    pub fixed_line_length: Option<String>,

    #[structopt(long = "fd-lines-as-code", global = true, help = "Compile fixed-form D lines instead of ignoring them")]
    pub d_lines_as_code: bool,

    #[structopt(long = "cpp", global = true,
                help = "Apply preprocessor conditionals (default for .F, .F90 and other upper-case extensions)")]
    pub cpp: bool,

    #[structopt(long = "nocpp", global = true, conflicts_with = "cpp", help = "Do not apply preprocessor conditionals")]
    pub nocpp: bool,

    #[structopt(long = "define", number_of_values = 1, global = true, value_name = "NAME[=VALUE]",
                help = "Define a preprocessor name, as -DNAME or -DNAME=VALUE")]
    pub defines: Vec<String>,

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
    /// Start an interactive scalar Fortran session.
    Repl,
    #[structopt(setting = structopt::clap::AppSettings::Hidden)]
    ReplEval { input: PathBuf },
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

/// The first argument that is neither an option nor an option's value: a
/// subcommand or the first source.
fn first_positional(args: &[std::ffi::OsString]) -> Option<String> {
    // Options whose value may be the next argument.
    const VALUED: &[&str] = &["-o", "--out", "-O", "--opt-level", "-I", "--module-path", "-J", "--module-dir",
                              "--target", "--std", "--warning", "--ffixed-line-length", "--define"];
    let mut args = args.iter().skip(1).map(|a| a.to_string_lossy());
    while let Some(arg) = args.next() {
        if arg == "--" {
            return args.next().map(|a| a.into_owned());
        }
        if VALUED.contains(&arg.as_ref()) {
            args.next();
        } else if !arg.starts_with('-') || arg == "-" {
            return Some(arg.into_owned());
        }
    }
    None
}

impl Cli {
    pub fn from_command_line() -> Self {
        let mut args = Vec::<std::ffi::OsString>::new();
        let mut original=std::env::args_os().collect::<Vec<_>>();
        let mut run_args=Vec::new();
        if let Some(split)=original.iter().position(|a|a=="--") {
            if original[..split].iter().any(|a|a=="--run"||a=="run") {
                run_args=original.split_off(split+1);original.pop();
            }
        }
        for arg in original {
            let text = arg.to_string_lossy();
            match text.as_ref() {
                "-std" => args.push("--std".into()),
                flag if flag.starts_with("-std=") => args.push(format!("--std={}",&flag[5..]).into()),
                "-Wall" | "--wall" => args.push("--Wall".into()),
                "-Werror" | "--werror" => args.push("--Werror".into()),
                "-ffixed-form" | "-ffree-form" | "-fd-lines-as-code" => args.push(format!("-{text}").into()),
                "-fd-lines-as-comments" => {}
                "-cpp" | "-nocpp" => args.push(format!("-{text}").into()),
                "-D" => args.push("--define".into()),
                flag if flag.starts_with("-D") => {
                    args.push("--define".into());
                    args.push(flag[2..].into());
                }
                flag if flag.starts_with("-ffixed-line-length") => {
                    // gfortran spells it -ffixed-line-length-N; accept =N too.
                    let value = flag["-ffixed-line-length".len()..].trim_start_matches(['-', '=']);
                    args.push(format!("--ffixed-line-length={value}").into());
                }
                flag if flag.starts_with("-W") => {
                    args.push("--warning".into());
                    args.push(flag[2..].into());
                }
                _ => args.push(arg),
            }
        }
        let mut app = Self::clap();
        if first_positional(&args).is_some_and(|a| a.contains(['.', '/', '\\'])) {
            // clap rejects a source named like a subcommand, such as run.f90
            // or hello.f, as a mistyped subcommand. Inferring subcommands turns
            // that check off; no subcommand name contains these characters,
            // so a source cannot be inferred as one.
            app = app.setting(structopt::clap::AppSettings::InferSubcommands);
        }
        let mut cli=Self::from_clap(&app.get_matches_from(args));cli.run_args=run_args;cli
    }

    /// Whether `path` is fixed source form: by option, else by extension.
    pub fn is_fixed_form(&self, path: &std::path::Path) -> bool {
        if self.fixed_form || self.free_form {
            return self.fixed_form;
        }
        let extension = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
        crate::utils::FIXED_FORM_EXTENSIONS.contains(&extension.as_str())
    }

    pub fn fixed_form_options(&self) -> anyhow::Result<fixed2free::Options> {
        let line_length = match self.fixed_line_length.as_deref() {
            None => Some(72),
            Some("none" | "0") => None,
            Some(value) => match value.parse::<usize>() {
                Ok(n) if n >= 7 => Some(n),
                _ => anyhow::bail!("invalid fixed-form line length `{value}`; expected a number of at least 7, or none"),
            },
        };
        Ok(fixed2free::Options { line_length, debug_lines_as_code: self.d_lines_as_code, ..Default::default() })
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
