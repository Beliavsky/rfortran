#![allow(dead_code, unused_imports)]
include!("frontend_modules.rs");
use frontend::{prepare_program, lower_program_body, suppress_legacy_unused};
use anyhow::{bail, Result};
use structopt::StructOpt;

fn run_program() -> Result<()> {
    let path = std::path::Path::new("/program.f90");
    let args = cli::Cli::from_iter_safe(["rfort", "run", "/program.f90"])?;
    let input = source_includes::read(path, &args)?;
    let tokens = lexer::lex(&input.text);
    let parsed = parser::parse_with_origins(&input.text, &tokens, "program.f90", Some(&input.origins))
        .map_err(|_| anyhow::anyhow!("parse failed"))?;
    let mut settings = sema::parse_directives(&input.text);
    let prepared = prepare_program(parsed, &input.text, &tokens, path, Some(&input.origins), &settings, &args)?;
    let tokens = lexer::lex(&prepared.source);
    let lowered = lower_program_body(prepared.program, &prepared.source, &tokens, "program.f90", Some(&prepared.origins))?;
    suppress_legacy_unused(&mut settings);
    let errors = sema::analyze_with_origins(&lowered, &prepared.source, &tokens, "program.f90", &settings,
        &std::collections::HashSet::new(), false, Some(&prepared.origins));
    for error in &errors {eprintln!("{error}");}
    if errors.iter().any(|e| matches!(e.kind, errors::CompileErrorKind::Semantic)) {bail!("semantic checks failed");}
    let program = initialized::instrument_with_origins(lowered, &prepared.source, "program.f90", &prepared.origins)?;
    runtime::interpret(&program);
    Ok(())
}
fn main() {
    let result=if std::env::args().nth(1).as_deref()==Some("repl") {
        cli::Cli::from_iter_safe(["rfort","repl"]).map_err(anyhow::Error::from)
            .and_then(|args|repl::browser::run(&args))
    }else{run_program()};
    if let Err(error) = result {
        eprintln!("Error: {}", source_diagnostics::user_names(&format!("{error:#}")));
        std::process::exit(1);
    }
}
