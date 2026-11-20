#![allow(dead_code, unused_imports)]
include!("frontend_modules.rs");
use frontend::{prepare_program, lower_program_body, suppress_legacy_unused};
use anyhow::{bail, Result};
use structopt::StructOpt;

/// Run, or with `check` only check, `/program.f90`. `options` are the
/// warning options chosen on the page, spelled as on the command line
/// (`-Wall`, `-Wextra`, `-Wstyle`, ...).
fn run_program(check: bool, options: &[String]) -> Result<()> {
    let path = std::path::Path::new("/program.f90");
    if let Some(other) = options.iter().find(|o| !o.starts_with("-W") || o.len() < 3) {
        bail!("the playground accepts only warning options such as -Wall; `{other}` is not one");
    }
    let mut command: Vec<std::ffi::OsString> = vec!["rfort".into()];
    command.extend(options.iter().map(Into::into));
    command.extend(["run".into(), "/program.f90".into()]);
    let args = cli::Cli::from_iter_safe(cli::Cli::gcc_style_options(command))?;
    let mut input = source_includes::read(path, &args)?;
    // The playground runs a program without its final END, which Fortran
    // requires, and says so. A construct left open is still an error.
    if parser::missing_closers(&lexer::lex(&input.text), input.text.len()).as_deref() == Some(&["END"]) {
        eprintln!("warning: missing END statement; `end` is assumed at the end of the source");
        if !input.text.ends_with('\n') { input.text.push('\n'); }
        input.text.push_str("end\n");
    }
    let tokens = lexer::lex(&input.text);
    let parsed = parser::parse_with_origins(&input.text, &tokens, "program.f90", Some(&input.origins))
        .map_err(|_| anyhow::anyhow!("parse failed"))?;
    let mut settings = sema::parse_directives(&input.text);
    settings.wall |= args.wall;
    settings.werror |= args.werror;
    let prepared = prepare_program(parsed, &input.text, &tokens, path, Some(&input.origins), &settings, &args)?;
    let tokens = lexer::lex(&prepared.source);
    let lowered = lower_program_body(prepared.program, &prepared.source, &tokens, "program.f90", Some(&prepared.origins))?;
    suppress_legacy_unused(&mut settings);
    let errors = sema::analyze_with_origins(&lowered, &prepared.source, &tokens, "program.f90", &settings,
        &std::collections::HashSet::new(), false, Some(&prepared.origins));
    for error in &errors {eprintln!("{error}");}
    if errors.iter().any(|e| matches!(e.kind, errors::CompileErrorKind::Semantic)) {bail!("semantic checks failed");}
    if check {
        println!("No errors found. The program was checked, not run.");
        return Ok(());
    }
    let program = initialized::instrument_with_origins(lowered, &prepared.source, "program.f90", &prepared.origins)?;
    runtime::interpret(&program);
    Ok(())
}
fn main() {
    let result=if std::env::args().nth(1).as_deref()==Some("repl") {
        cli::Cli::from_iter_safe(["rfort","repl"]).map_err(anyhow::Error::from)
            .and_then(|args|repl::browser::run(&args))
    }else{
        // rfort-browser [check] [warning options...]
        let mut rest: Vec<String> = std::env::args().skip(1).collect();
        let check = rest.first().map(String::as_str) == Some("check");
        if check { rest.remove(0); }
        run_program(check, &rest)
    };
    if let Err(error) = result {
        eprintln!("Error: {}", private_entities::with_notes(source_diagnostics::user_names(&format!("{error:#}"))));
        std::process::exit(1);
    }
}
