include!("frontend_modules.rs");
use frontend::{prepare_program, lower_program_body, suppress_legacy_unused};
use crate::codegen::Backend;
use anyhow::Result;
use cranelift_codegen::settings::{self, Configurable, Flags};

mod driver;
mod codegen;
mod ir;
mod command_time;
mod linker;
#[cfg(test)]
#[path = "parallel_runtime.rs"]
mod parallel_runtime_tests;

#[cfg(all(target_os = "windows", feature = "bundled-objdump"))]
const OBJDUMP_BYTES: Option<&[u8]> = Some(include_bytes!("bin/tools/objdump.exe"));
#[cfg(not(all(target_os = "windows", feature = "bundled-objdump")))]
const OBJDUMP_BYTES: Option<&[u8]> = None;

#[derive(Debug, Clone)]
struct BuildArtifact {
    object: std::path::PathBuf,
    has_program: bool,
}

fn main() -> Result<()> {
    let started = std::time::Instant::now();
    // Windows' small default main-thread stack is insufficient for the AST
    // traversals needed by valid high-rank array expressions.
    match std::thread::Builder::new().name("rfort".into()).stack_size(32*1024*1024).spawn(move || compiler_main(started))?.join() {
        // Report as returning the error from main would, with user names.
        Ok(Err(error))=>{
            if let Some(exit) = error.downcast_ref::<command_time::ProgramExit>() {
                std::process::exit(exit.0);
            }
            eprintln!("Error: {}", source_diagnostics::user_names(&format!("{error:?}")));
            std::process::exit(1);
        }
        Ok(result)=>result,
        Err(panic)=>std::panic::resume_unwind(panic),
    }
}

fn compiler_main(started: std::time::Instant) -> Result<()> {
    env_logger::init();
    let args = cli::Cli::from_command_line().normalize();

    concurrent_reduce::parallel::configure(args.parallel && matches!(args.cmd,
        Some(cli::Command::Build { .. } | cli::Command::EmitObj { .. })), args.parallel_report);
    if args.parallel && !matches!(args.cmd, Some(cli::Command::Build { .. } | cli::Command::EmitObj { .. } | cli::Command::Link { .. })) {
        anyhow::bail!("--parallel is available for native compilation; the interpreter remains serial");
    }
    let _overall = command_time::Overall::new(args.time, started);
    unused::validate_options(&args.warnings)?;

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
        cli::Command::Repl => repl::run(&args)?,
        cli::Command::ReplEval { input } => repl::evaluate(&input)?,
        cli::Command::Lex { input, out } => {
            let source_input = source_includes::read(&input, &args)?;
            let src = source_input.text;
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
            let source_input = source_includes::read(&input, &args)?;
            let src = source_input.text;
            let tokens = lexer::lex(&src);
            match parser::parse_with_origins(&src, &tokens, input.to_str().unwrap_or("<unknown>"), Some(&source_input.origins)) {
                Ok(p) => {
                    standards::check(&p, &src, &tokens, input.to_str().unwrap_or("<unknown>"), Some(&source_input.origins), args.standard, args.werror)?;
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
                // The parser has reported each error.
                Err(_) => {
                    anyhow::bail!("compilation failed");
                }
            }
        }
        cli::Command::Check { input } => {
            let source_input = source_includes::read(&input, &args)?;
            let src = source_input.text;
            let tokens = lexer::lex(&src);
            let settings_from_src = sema::parse_directives(&src);
            let mut settings = settings_from_src;
            settings.wall |= args.wall;
            settings.werror |= args.werror;
            let mut any_err = false;
            let program = match parser::parse_with_origins(
                &src,
                &tokens,
                input.to_str().unwrap_or("<unknown>"),
                Some(&source_input.origins),
            ) {
                Ok(p) => p,
                // The parser has reported each error.
                Err(_) => {
                    any_err = true;
                    crate::ast::Program {
                        name: "<error>".into(),
                        body: vec![],
                        contains: vec![],
                    }
                }
            };
            let prepared = prepare_program(program, &src, &tokens, &input, Some(&source_input.origins), &settings, &args)?;
            let src = &prepared.source;
            let tokens = lexer::lex(src);
            let program = lower_program_body(prepared.program.clone(), src, &tokens,
                input.to_str().unwrap_or("<unknown>"), Some(&prepared.origins))?;
            suppress_legacy_unused(&mut settings);
            let empty_obj_exports = std::collections::HashSet::new();
            let sema_errs = sema::analyze_with_origins(
                &program,
                &src,
                &tokens,
                input.to_str().unwrap_or("<unknown>"),
                &settings,
                &empty_obj_exports,
                false,
                Some(&prepared.origins),
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
                    anyhow::bail!("compilation failed");
                }
            }
            if !any_err { initialized::instrument_with_origins(program, &src, input.file_name().and_then(|name| name.to_str()).unwrap_or("<unknown>"), &prepared.origins)?; }
            if sema_errs.is_empty() && !any_err {
                println!("No problems found.");
            }
            if any_err { anyhow::bail!("parse failed"); }
        }
        cli::Command::Build { inputs, out, run } => {
            driver::build(&inputs, out.or_else(|| args.out.clone()), run || args.run, false, &args)?;
        }
        cli::Command::EmitObj { input, out } => {
            let mut timing = command_time::Phase::new(args.time, "Compilation");
            let art = compile_to_object(&input, &out, args.wall, args.werror, &args, false, None)?;
            timing.finish(true);
            if !args.quiet {
                println!("Wrote object: {}", art.object.display());
            }
        }
        cli::Command::Run { input } => {
            let source_input = source_includes::read(&input, &args)?;
            let src = source_input.text;
            let tokens = lexer::lex(&src);
            let settings_from_src = sema::parse_directives(&src);
            let mut settings = settings_from_src;
            settings.wall |= args.wall;
            settings.werror |= args.werror;
            let program = match parser::parse_with_origins(
                &src,
                &tokens,
                input.to_str().unwrap_or("<unknown>"),
                Some(&source_input.origins),
            ) {
                Ok(p) => p,
                // The parser has reported each error.
                Err(_) => {
                    anyhow::bail!("parse failed");
                }
            };
            let prepared = prepare_program(program, &src, &tokens, &input, Some(&source_input.origins), &settings, &args)?;
            let src = &prepared.source;
            let tokens = lexer::lex(src);
            let program = lower_program_body(prepared.program.clone(), src, &tokens,
                input.to_str().unwrap_or("<unknown>"), Some(&prepared.origins))?;
            suppress_legacy_unused(&mut settings);
            let empty_obj_exports = std::collections::HashSet::new();
            let sema_errs = sema::analyze_with_origins(
                &program,
                &src,
                &tokens,
                input.to_str().unwrap_or("<unknown>"),
                &settings,
                &empty_obj_exports,
                false,
                Some(&prepared.origins),
            );
            if !sema_errs.is_empty() {
                for e in &sema_errs {
                    eprintln!("{}", e);
                }
            }
            if sema_errs.iter().any(|e| matches!(e.kind, crate::errors::CompileErrorKind::Semantic)) {
                anyhow::bail!("semantic checks failed");
            }
            let program=integer_arithmetic::resolve(program,true,&src,&prepared.origins)?;
            let program = initialized::instrument_with_origins(program, &src, input.file_name().and_then(|name| name.to_str()).unwrap_or("<unknown>"), &prepared.origins)?;
            let command=std::iter::once(input.as_os_str()).chain(args.run_args.iter().map(|a|a.as_os_str()))
                .map(|a|std::ffi::CString::new(a.to_string_lossy().as_bytes())).collect::<std::result::Result<Vec<_>,_>>()?;
            let pointers=command.iter().map(|a|a.as_ptr().cast::<u8>()).collect::<Vec<_>>();
            unsafe {environment::rfort_set_arguments(i32::try_from(pointers.len())?,pointers.as_ptr());}
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
    origins: Option<&[source_diagnostics::SourceOrigin]>,
) -> Result<BuildArtifact> {
    // A combined source was converted file by file already.
    let source_input = if let Some(origins) = origins {
        submodules::merge(source_includes::Input { text: utils::read_file_to_string(input)?, origins: origins.to_vec() })?
    } else { source_includes::read(input, args)? };
    let src = source_input.text;
    let origins = Some(source_input.origins.as_slice());
    let tokens = lexer::lex(&src);
    let settings_from_src = sema::parse_directives(&src);
    let mut settings = settings_from_src;
    settings.wall |= wall;
    settings.werror |= werror;
    let parse_result = compile_profile::run("parse", || parser::parse_with_origins(&src, &tokens, input.to_str().unwrap_or("<unknown>"), Some(&source_input.origins)));
    let program = match parse_result {
        Ok(p) => p,
        // The parser has reported each error.
        Err(_) => {
            return Err(anyhow::anyhow!("parse failed"));
        }
    };
    let prepared = compile_profile::run("prepare_program", || prepare_program(program, &src, &tokens, input, origins, &settings, args))?;
    let src = &prepared.source;
    let tokens = lexer::lex(src);
    let program = lower_program_body(prepared.program.clone(), src, &tokens,
        input.to_str().unwrap_or("<unknown>"), Some(&prepared.origins))?;
    suppress_legacy_unused(&mut settings);
    let mut empty_obj_exports = std::collections::HashSet::new();
    if args.compile_only {
        // An implicit external subroutine interface can be resolved at link
        // time. Do not suppress unrelated undefined-variable diagnostics.
        fn external_calls(body: &[ast::Stmt], names: &mut std::collections::HashSet<String>) {
            for statement in body {
                match statement {
                    ast::Stmt::CallSub { name, .. } if !name.starts_with('@') => { names.insert(name.clone()); }
                    ast::Stmt::Module { body, .. } | ast::Stmt::Function { body, .. }
                    | ast::Stmt::Subroutine { body, .. } | ast::Stmt::Block { body }
                    | ast::Stmt::Do { body, .. } | ast::Stmt::DoWhile { body, .. } => external_calls(body, names),
                    ast::Stmt::If { then_body, else_body, .. } => {
                        external_calls(then_body, names);
                        if let Some(body) = else_body { external_calls(body, names); }
                    }
                    ast::Stmt::SelectCase { cases, default, .. } => {
                        for case in cases { external_calls(&case.body, names); }
                        if let Some(body) = default { external_calls(body, names); }
                    }
                    _ => {}
                }
            }
        }
        external_calls(&program.body, &mut empty_obj_exports);
    }
    let sema_errs = compile_profile::run("sema", || sema::analyze_with_origins(
        &program,
        &src,
        &tokens,
        input.to_str().unwrap_or("<unknown>"),
        &settings,
        &empty_obj_exports,
        allow_undefined,
        Some(&prepared.origins),
    ));
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
    let program = integer_arithmetic::resolve(program,!args.nocheck,&src,&prepared.origins)
        .map_err(|e|source_diagnostics::report(e,&src,&tokens,input.to_str().unwrap_or("<unknown>"),Some(&prepared.origins)))?;
    let program = compile_profile::run("initialized", || initialized::instrument_with_origins(program, &src, input.file_name().and_then(|name| name.to_str()).unwrap_or("<unknown>"), &prepared.origins))?;
    let program = if args.nocheck { nocheck::strip(program) } else { program };
    let lowered = compile_profile::run("ir", || ir::lower_to_ir_with_debug(&program, args.debug_ir))?;
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
    let backend = codegen::cranelift::CraneliftBackend::with_flags(flags)
        .with_module_owners(prepared.owned.clone());
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
    prepared.publish()?;
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
