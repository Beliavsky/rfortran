// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! Command-line front end of the fixed-to-free source form converter.

use fixed2free::{convert, Hollerith, Options};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const USAGE: &str = "\
usage: fixed2free [options] FILE...

Convert Fortran fixed source form to free source form. Each FILE is written
beside itself with the extension .f90 (.F90 for .F, .FOR and .FPP files),
keeping its line numbers.

options:
  -o FILE               write the single input FILE here; - writes standard output
  --suffix EXT          extension of the outputs (default f90, or F90)
  --force               overwrite existing outputs
  --line-length N|none  last column of the statement field (default 72)
  --debug-lines         compile lines with D in column 1 instead of commenting them
  --hollerith format|all|none
                        which Hollerith constants become character literals
                        (default format: only in FORMAT statements)
  -q, --quiet           do not print warnings
  -h, --help            show this help
";

struct Arguments {
    inputs: Vec<PathBuf>,
    output: Option<String>,
    suffix: Option<String>,
    force: bool,
    quiet: bool,
    options: Options,
}

fn parse(mut args: impl Iterator<Item = String>) -> Result<Arguments, String> {
    let mut parsed = Arguments { inputs: vec![], output: None, suffix: None, force: false, quiet: false, options: Options::default() };
    while let Some(arg) = args.next() {
        let mut value = |name: &str| args.next().ok_or_else(|| format!("{name} requires a value"));
        match arg.as_str() {
            "-h" | "--help" => return Err(String::new()),
            "-o" => parsed.output = Some(value("-o")?),
            "--suffix" => parsed.suffix = Some(value("--suffix")?.trim_start_matches('.').to_string()),
            "--force" => parsed.force = true,
            "-q" | "--quiet" => parsed.quiet = true,
            "--debug-lines" => parsed.options.debug_lines_as_code = true,
            "--line-length" => {
                let length = value("--line-length")?;
                parsed.options.line_length = match length.as_str() {
                    "none" => None,
                    n => Some(n.parse().ok().filter(|n| *n > 6).ok_or_else(|| format!("invalid line length `{n}`"))?),
                };
            }
            "--hollerith" => {
                parsed.options.hollerith = match value("--hollerith")?.as_str() {
                    "format" => Hollerith::Format,
                    "all" => Hollerith::All,
                    "none" => Hollerith::None,
                    other => return Err(format!("invalid --hollerith `{other}`; expected format, all or none")),
                };
            }
            option if option.starts_with('-') && option != "-" => return Err(format!("unknown option `{option}`")),
            input => parsed.inputs.push(PathBuf::from(input)),
        }
    }
    if parsed.inputs.is_empty() {
        return Err("no input files".into());
    }
    if parsed.output.is_some() && parsed.inputs.len() > 1 {
        return Err("-o takes a single input file".into());
    }
    Ok(parsed)
}

/// The output beside `input`: a preprocessed (uppercase) source stays preprocessed.
fn beside(input: &Path, suffix: Option<&str>) -> PathBuf {
    let extension = input.extension().and_then(|e| e.to_str()).unwrap_or("");
    let suffix = suffix.unwrap_or(if matches!(extension, "F" | "FOR" | "FPP") { "F90" } else { "f90" });
    input.with_extension(suffix)
}

fn main() -> ExitCode {
    let arguments = match parse(std::env::args().skip(1)) {
        Ok(arguments) => arguments,
        Err(message) => {
            if message.is_empty() {
                print!("{USAGE}");
                return ExitCode::SUCCESS;
            }
            eprintln!("fixed2free: {message}\n\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    let mut failed = false;
    for input in &arguments.inputs {
        let source = match std::fs::read(input) {
            Ok(source) => source,
            Err(error) => {
                eprintln!("fixed2free: {}: {error}", input.display());
                failed = true;
                continue;
            }
        };
        let converted = convert(&source, &arguments.options);
        if !arguments.quiet {
            for warning in &converted.warnings {
                if warning.line == 0 {
                    eprintln!("{}: warning: {}", input.display(), warning.message);
                } else {
                    eprintln!("{}:{}: warning: {}", input.display(), warning.line, warning.message);
                }
            }
        }
        if arguments.output.as_deref() == Some("-") {
            use std::io::Write;
            if std::io::stdout().write_all(&converted.text).is_err() {
                return ExitCode::FAILURE;
            }
            continue;
        }
        let output = arguments.output.as_ref().map(PathBuf::from).unwrap_or_else(|| beside(input, arguments.suffix.as_deref()));
        if output == *input {
            eprintln!("fixed2free: {}: output would replace the input", input.display());
            failed = true;
            continue;
        }
        if output.exists() && !arguments.force {
            eprintln!("fixed2free: {}: exists; use --force to overwrite", output.display());
            failed = true;
            continue;
        }
        if let Err(error) = std::fs::write(&output, &converted.text) {
            eprintln!("fixed2free: {}: {error}", output.display());
            failed = true;
        }
    }
    if failed { ExitCode::FAILURE } else { ExitCode::SUCCESS }
}
