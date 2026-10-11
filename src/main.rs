use crate::codegen::Backend;
use anyhow::Result;
use cranelift_codegen::settings::{self, Configurable, Flags};
use structopt::StructOpt;

mod ast;
mod cli;
mod driver;
mod codegen;
mod errors;
mod ir;
mod intrinsics;
mod bit_intrinsics;
mod inquiries;
mod lexer;
mod linker;
mod parser;
mod parameters;
mod sections;
mod reductions;
mod initialized;
mod input;
mod file_io;
mod output;
mod do_steps;
mod purity;
mod elemental;
mod subroutines;
mod array_calls;
mod allocatables;
mod optional;
mod named_arguments;
mod substrings;
mod character_arrays;
mod vector_subscripts;
mod function_arrays;
mod array_bounds;
mod multidim;
mod formatting;
mod format_spec;
mod record_input;
mod character_input;
mod derived;
mod operators;
mod imports;
mod runtime;
mod sema;
mod utils;

#[cfg(target_os = "windows")]
const OBJDUMP_BYTES: Option<&[u8]> = Some(include_bytes!("bin/tools/objdump.exe"));
#[cfg(not(target_os = "windows"))]
const OBJDUMP_BYTES: Option<&[u8]> = None;

#[derive(Debug, Clone)]
struct BuildArtifact {
    object: std::path::PathBuf,
    has_program: bool,
}

fn main() -> Result<()> {
    env_logger::init();
    let args = cli::Cli::from_args().normalize();

    if args.help {
        utils::print_help();
        return Ok(());
    }

    if args.version {
        let version = env!("CARGO_PKG_VERSION");
        println!("rfort version: {}", version);
        println!("Host target: {}", target_lexicon::Triple::host());
        return Ok(());
    }

    if let Some(target) = &args.target {
        if target != &target_lexicon::Triple::host().to_string() {
            anyhow::bail!("cross-target compilation is not yet supported; build and run rfort on the target platform");
        }
    }
    match args.cmd.clone().unwrap_or(cli::Command::Help) {
        cli::Command::Lex { input, out } => {
            let src = utils::read_file_to_string(&input)?;
            let tokens = lexer::lex(&src);
            let mut serialized = String::new();
            let mut longest_token = 0;
            for t in &tokens {
                let len = format!("{:?}", t.kind).len();
                if len > longest_token {
                    longest_token = len;
                }
            }
            for t in &tokens {
                let token_str = format!("{:?}", t.kind);
                let padding = " ".repeat(longest_token - token_str.len());
                serialized.push_str(&format!(
                    "{}{} | @ {}..{}\n",
                    token_str, padding, t.span.start, t.span.end
                ));
            }
            if let Some(out_path) = out {
                std::fs::write(&out_path, serialized)?;
                if !args.quiet {
                    println!("Wrote tokens to {}", out_path.display());
                }
            } else {
                println!("{}", serialized);
            }
        }
        cli::Command::Parse { input, out } => {
            let src = utils::read_file_to_string(&input)?;
            let tokens = lexer::lex(&src);
            match parser::parse_with_src(&src, &tokens, input.to_str().unwrap_or("<unknown>")) {
                Ok(p) => {
                    if let Some(out_path) = out {
                        let serialized = serde_json::to_string_pretty(&p)?;
                        std::fs::write(&out_path, serialized)?;
                        if !args.quiet {
                            println!("Wrote AST to {}", out_path.display());
                        }
                    } else {
                        println!("{:#?}", p);
                    }
                }
                Err(errs) => {
                    for e in errs {
                        eprintln!("{}", e);
                    }
                    std::process::exit(1);
                }
            }
        }
        cli::Command::Check { input } => {
            let src = utils::read_file_to_string(&input)?;
            let tokens = lexer::lex(&src);
            let settings_from_src = sema::parse_directives(&src);
            let mut settings = settings_from_src;
            settings.wall |= args.wall;
            settings.werror |= args.werror;
            let mut any_err = false;
            let program = match parser::parse_with_src(
                &src,
                &tokens,
                input.to_str().unwrap_or("<unknown>"),
            ) {
                Ok(p) => p,
                Err(errs) => {
                    any_err = true;
                    for e in &errs {
                        eprintln!("{}", e);
                    }
                    crate::ast::Program {
                        name: "<error>".into(),
                        body: vec![],
                    }
                }
            };
            let program = sections::resolve(reductions::resolve(array_calls::resolve(parameters::resolve(derived::resolve(program)?)?)?)?)?;
            let empty_obj_exports = std::collections::HashSet::new();
            let sema_errs = sema::analyze_with_src(
                &program,
                &src,
                &tokens,
                input.to_str().unwrap_or("<unknown>"),
                &settings,
                &empty_obj_exports,
                false,
            );
            if !sema_errs.is_empty() {
                let mut has_error = false;
                for e in &sema_errs {
                    eprintln!("{}", e);
                    if matches!(e.kind, crate::errors::CompileErrorKind::Semantic) {
                        has_error = true;
                    }
                }
                if has_error || settings.werror {
                    std::process::exit(1);
                }
            }
            if !any_err { initialized::instrument_with_source(program, &src, input.file_name().and_then(|name| name.to_str()).unwrap_or("<unknown>"))?; }
            if sema_errs.is_empty() && !any_err {
                println!("No problems found.");
            }
            if any_err { anyhow::bail!("parse failed"); }
        }
        cli::Command::Build { inputs, out, run } => {
            driver::build(&inputs, out.or_else(|| args.out.clone()), run || args.run, false, &args)?;
        }
        cli::Command::EmitObj { input, out } => {
            let art = compile_to_object(&input, &out, args.wall, args.werror, &args, false)?;
            if !args.quiet {
                println!("Wrote object: {}", art.object.display());
            }
        }
        cli::Command::Run { input } => {
            let src = utils::read_file_to_string(&input)?;
            let tokens = lexer::lex(&src);
            let settings_from_src = sema::parse_directives(&src);
            let mut settings = settings_from_src;
            settings.wall |= args.wall;
            settings.werror |= args.werror;
            let program = match parser::parse_with_src(
                &src,
                &tokens,
                input.to_str().unwrap_or("<unknown>"),
            ) {
                Ok(p) => p,
                Err(errs) => {
                    for e in &errs {
                        eprintln!("{}", e);
                    }
                    crate::ast::Program {
                        name: "<error>".into(),
                        body: vec![],
                    }
                }
            };
            let program = sections::resolve(reductions::resolve(array_calls::resolve(parameters::resolve(derived::resolve(program)?)?)?)?)?;
            let empty_obj_exports = std::collections::HashSet::new();
            let sema_errs = sema::analyze_with_src(
                &program,
                &src,
                &tokens,
                input.to_str().unwrap_or("<unknown>"),
                &settings,
                &empty_obj_exports,
                false,
            );
            if !sema_errs.is_empty() {
                for e in &sema_errs {
                    eprintln!("{}", e);
                }
            }
            if sema_errs.iter().any(|e| matches!(e.kind, crate::errors::CompileErrorKind::Semantic)) {
                anyhow::bail!("semantic checks failed");
            }
            let program = initialized::instrument_with_source(program, &src, input.file_name().and_then(|name| name.to_str()).unwrap_or("<unknown>"))?;
            runtime::interpret(&program);
        }
        cli::Command::Link { inputs, out } => {
            driver::build(&inputs, out.or_else(|| args.out.clone()), args.run, true, &args)?;
        }
        cli::Command::Help => {
            utils::print_help();
        }
    }
    Ok(())
}

fn compile_to_object(
    input: &std::path::Path,
    out: &std::path::Path,
    wall: bool,
    werror: bool,
    args: &cli::Cli,
    allow_undefined: bool,
) -> Result<BuildArtifact> {
    let src = utils::read_file_to_string(input)?;
    let tokens = lexer::lex(&src);
    let settings_from_src = sema::parse_directives(&src);
    let mut settings = settings_from_src;
    settings.wall |= wall;
    settings.werror |= werror;
    let parse_result = parser::parse_with_src(&src, &tokens, input.to_str().unwrap_or("<unknown>"));
    let program = match parse_result {
        Ok(p) => p,
        Err(errs) => {
            for e in &errs {
                eprintln!("{}", e);
            }
            return Err(anyhow::anyhow!("parse failed"));
        }
    };
    let program = sections::resolve(reductions::resolve(array_calls::resolve(parameters::resolve(derived::resolve(program)?)?)?)?)?;
    let empty_obj_exports = std::collections::HashSet::new();
    let sema_errs = sema::analyze_with_src(
        &program,
        &src,
        &tokens,
        input.to_str().unwrap_or("<unknown>"),
        &settings,
        &empty_obj_exports,
        allow_undefined,
    );
    if !sema_errs.is_empty() {
        let mut has_error = false;
        for e in &sema_errs {
            if allow_undefined && e.message.contains("undefined") {
                continue;
            }
            eprintln!("{}", e);
            if matches!(e.kind, crate::errors::CompileErrorKind::Semantic) {
                has_error = true;
            }
        }
        if has_error || settings.werror {
            return Err(anyhow::anyhow!("compile failed"));
        }
    }
    let program = initialized::instrument_with_source(program, &src, input.file_name().and_then(|name| name.to_str()).unwrap_or("<unknown>"))?;
    let lowered = ir::lower_to_ir_with_debug(&program, args.debug_ir)?;
    let has_program = lowered.has_program;
    if !args.quiet {
        if !lowered.defines_modules.is_empty() { eprintln!("Defines modules: {}", lowered.defines_modules.join(", ")); }
        if !lowered.uses_modules.is_empty() { eprintln!("Uses modules: {}", lowered.uses_modules.join(", ")); }
    }
    let module = lowered.module;
    let mut flag_builder = settings::builder();
    // Touch each guard page before using a large generated stack frame.
    flag_builder.set("enable_probestack", "true")?;
    flag_builder.set("probestack_strategy", "inline")?;
    match args.opt_level.as_str() {
        "0" => {
            flag_builder.set("opt_level", "none").ok();
        }
        "1" => {
            flag_builder.set("opt_level", "speed").ok();
        }
        "2" => {
            flag_builder.set("opt_level", "speed").ok();
            flag_builder.set("enable_gvn", "true").ok();
            flag_builder.set("enable_licm", "true").ok();
        }
        "3" => {
            flag_builder.set("opt_level", "speed").ok();
            flag_builder.set("enable_verifier", "false").ok();
            flag_builder.set("enable_gvn", "true").ok();
            flag_builder.set("enable_licm", "true").ok();
            flag_builder.set("enable_preopt", "true").ok();
            flag_builder.set("enable_postopt", "true").ok();
        }
        "s" => {
            flag_builder.set("opt_level", "speed_and_size").ok();
            flag_builder.set("enable_gvn", "true").ok();
            flag_builder.set("enable_licm", "true").ok();
            flag_builder.set("preserve_frame_pointers", "false").ok();
            flag_builder.set("unwind_info", "false").ok();
        }
        "S" => {
            flag_builder.set("opt_level", "speed_and_size").ok();
            flag_builder.set("enable_gvn", "true").ok();
            flag_builder.set("enable_licm", "true").ok();
            flag_builder.set("enable_preopt", "true").ok();
            flag_builder.set("enable_postopt", "true").ok();
            flag_builder.set("preserve_frame_pointers", "false").ok();
            flag_builder.set("unwind_info", "false").ok();
            flag_builder.set("machine_code_cfg_info", "false").ok();
        }
        "z" => {
            flag_builder.set("opt_level", "speed_and_size").ok();
            flag_builder.set("enable_gvn", "false").ok();
            flag_builder.set("enable_licm", "false").ok();
            flag_builder.set("enable_preopt", "true").ok();
            flag_builder.set("enable_postopt", "true").ok();
            flag_builder.set("enable_unroll_loops", "false").ok();
            flag_builder.set("preserve_frame_pointers", "false").ok();
            flag_builder.set("unwind_info", "false").ok();
            flag_builder.set("machine_code_cfg_info", "false").ok();
        }
        "Z" => {
            flag_builder.set("opt_level", "speed_and_size").ok();
            flag_builder.set("enable_gvn", "false").ok();
            flag_builder.set("enable_licm", "false").ok();
            flag_builder.set("enable_preopt", "true").ok();
            flag_builder.set("enable_postopt", "true").ok();
            flag_builder.set("enable_unroll_loops", "false").ok();
            flag_builder.set("preserve_frame_pointers", "false").ok();
            flag_builder.set("unwind_info", "false").ok();
            flag_builder.set("machine_code_cfg_info", "false").ok();
            flag_builder.set("regalloc_checker", "false").ok();
        }
        _ => {
            flag_builder.set("opt_level", "speed").ok();
        }
    }
    let flags = Flags::new(flag_builder);
    let backend = codegen::cranelift::CraneliftBackend::with_flags(flags);
    let obj = match backend.compile(&module, Some(&tokens)) {
        Ok(o) => o,
        Err(e) => {
            let msg = e.to_string();

            use codespan_reporting::diagnostic::{Diagnostic, Label};
            use codespan_reporting::files::SimpleFile;
            use codespan_reporting::term::termcolor::StandardStream;
            use codespan_reporting::term::{emit_to_io_write, Config};

            let mut stderr =
                StandardStream::stderr(codespan_reporting::term::termcolor::ColorChoice::Auto);
            let file = SimpleFile::new(input.to_str().unwrap_or("<unknown>"), &src);

            let mut labels: Vec<Label<()>> = Vec::new();
            if let Some(span_idx) = msg.find("@SPAN=") {
                let span_part = &msg[span_idx + 6..];
                if let Some(dots) = span_part.find("..") {
                    if let (Ok(s), Ok(ei)) = (
                        span_part[..dots].parse::<usize>(),
                        span_part[dots + 2..]
                            .split_whitespace()
                            .next()
                            .unwrap_or("")
                            .parse::<usize>(),
                    ) {
                        labels.push(Label::primary((), s..ei));
                    }
                }
            }

            let diag = if labels.is_empty() {
                Diagnostic::error().with_message(msg)
            } else {
                Diagnostic::error().with_message(msg).with_labels(labels)
            };

            let _ = emit_to_io_write(&mut stderr, &Config::default(), &file, &diag);
            return Err(anyhow::anyhow!("compile failed"));
        }
    };
    codegen::emitter::write_object_file(&obj, out)?;
    if args.emit_asm {
        let out_str = out.to_string_lossy().to_string();
        let mut printed = false;

        if let Some(bytes) = OBJDUMP_BYTES {
            let mut path = std::env::temp_dir();
            path.push("objdump.exe");

            if !path.exists() {
                std::fs::write(&path, bytes).expect("failed to write embedded objdump");
            }

            if let Ok(o) = std::process::Command::new(&path)
                .arg("-d")
                .arg(&out_str)
                .output()
            {
                println!("\n=== ASSEMBLY (via objdump) ===\n");
                let s = String::from_utf8_lossy(&o.stdout);
                println!("{}", s.replace(",", ", ")); // formate it nicely
                printed = true;
            }
        } else {
            let objdump = if cfg!(windows) {
                "objdump.exe"
            } else {
                "objdump"
            };

            if which::which(objdump).is_ok() {
                if let Ok(o) = std::process::Command::new(objdump)
                    .arg("-d")
                    .arg(&out_str)
                    .output()
                {
                    println!("\n=== ASSEMBLY (via {}) ===\n", objdump);
                    let s = String::from_utf8_lossy(&o.stdout);
                    println!("{}", s);
                    printed = true;
                }
            }

            if !printed && cfg!(windows) {
                if which::which("dumpbin.exe").is_ok() {
                    if let Ok(o) = std::process::Command::new("dumpbin.exe")
                        .arg("/DISASM")
                        .arg(&out_str)
                        .output()
                    {
                        println!("\n=== ASSEMBLY (via dumpbin) ===\n");
                        let s = String::from_utf8_lossy(&o.stdout);
                        println!("{}", s);
                        printed = true;
                    }
                }
            }
        }

        if !printed {
            eprintln!("Warning: no disassembler (embedded/system objdump or dumpbin) found; cannot emit assembly");
        }
    }
    Ok(BuildArtifact {
        object: out.to_path_buf(),
        has_program,
    })
}
