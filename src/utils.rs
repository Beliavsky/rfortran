use anyhow::Result;

// Exact INTEGER ** INTEGER, including Fortran's integer negative powers.
// None denotes an unrepresentable result or zero raised to a negative power.
pub fn checked_integer_power(mut base: i64, exponent: i64) -> Option<i64> {
    if exponent < 0 {
        return match base {
            0 => None,
            1 => Some(1),
            -1 => Some(if exponent % 2 == 0 { 1 } else { -1 }),
            _ => Some(0),
        };
    }
    let mut exponent = exponent as u64;
    let mut result = 1i64;
    while exponent != 0 {
        if exponent & 1 != 0 { result = result.checked_mul(base)?; }
        exponent >>= 1;
        if exponent != 0 { base = base.checked_mul(base)?; }
    }
    Some(result)
}

// Undelimited adjacent character values have no separator. Every record starts
// with a blank; all other value boundaries get one blank.
pub fn list_directed_separator(previous_character: Option<bool>, character: bool) -> &'static str {
    if previous_character == Some(true) && character { "" } else { " " }
}
use std::{
    fs,
    path::{Path, PathBuf},
};

pub fn read_file_to_string(path: &Path) -> Result<String> {
    Ok(fs::read_to_string(path)?.trim_start_matches('\u{feff}').to_owned())
}

/// Extensions of fixed source form files, in lower case.
pub const FIXED_FORM_EXTENSIONS: &[&str] = &["f", "for", "ftn", "f77", "fpp"];

/// Read a Fortran source, converting fixed source form to free form. The
/// conversion keeps every line on its line number, so diagnostics need no map.
pub fn read_source(path: &Path, cli: &crate::cli::Cli) -> Result<String> {
    let fixed = cli.is_fixed_form(path);
    // Preprocessor conditionals come first; their lines become blank.
    let preprocessed = if !cli.nocpp && (cli.cpp || crate::preprocess::by_extension(path)) {
        let text = read_file_to_string(path)?;
        let defined = crate::preprocess::definitions(&cli.defines)?;
        let text = crate::preprocess::preprocess(&text, &defined, fixed)
            .map_err(|e| anyhow::anyhow!("{}: {e:#}", path.display()))?;
        Some(text)
    } else {
        None
    };
    if !fixed {
        return match preprocessed {
            Some(text) => Ok(text),
            None => read_file_to_string(path),
        };
    }
    let options = cli.fixed_form_options()?;
    let bytes = match preprocessed {
        Some(text) => text.into_bytes(),
        None => fs::read(path)?,
    };
    let bytes = bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(&bytes);
    let converted = fixed2free::convert(bytes, &options);
    let mut warned = false;
    for warning in &converted.warnings {
        let shown = match warning.kind {
            // rfort has no free-form line length limit.
            fixed2free::WarningKind::FreeLineLength => false,
            // Sequence numbers after column 72 are common, so as gfortran
            // does, report the dropped text only with -Wall.
            fixed2free::WarningKind::Truncation => cli.wall,
            fixed2free::WarningKind::Source => true,
        };
        if !shown {
            continue;
        }
        warned = true;
        if warning.line == 0 {
            eprintln!("{}: Warning: {}", path.display(), warning.message);
        } else {
            eprintln!("{}:{}: Warning: {}", path.display(), warning.line, warning.message);
        }
    }
    if warned && cli.werror {
        anyhow::bail!("fixed source form warnings in {} treated as errors", path.display());
    }
    Ok(String::from_utf8_lossy(&converted.text).into_owned())
}

pub fn default_object_output_path(input: &Path) -> PathBuf {
    let mut out = input.to_path_buf();
    out.set_extension(if cfg!(target_os = "windows") {
        "obj"
    } else {
        "o"
    });
    out
}

pub fn print_help() {
    println!("Usage: rfort [OPTIONS] sources... | rfort <COMMAND> [ARGS]");
    println!("\nCommands:");
    println!("  lex <input>          Lex the input file and print tokens");
    println!("  parse <input>        Parse the input file and print the AST");
    println!("  check <input>        Check the input file for semantic errors");
    println!("  build <inputs>        Compile the input file to an object file and link it into an executable");
    println!("  emit-obj <input>     Compile the input file to an object file");
    println!("  run <input>          Parse, check, and run the input file");
    println!("  repl                 Start a persistent scalar Fortran session");
    println!("  link <inputs>        Link object files or sources into an executable");
    println!("\nOptions:");
    println!("  --verbose            Show build progress and toolchain commands");
    println!("  -c, --compile-only   Emit an object without linking");
    println!("  -I <directory>       Search for rfort .rmod module interfaces (repeatable)");
    println!("  -J <directory>       Write module interfaces here and search this directory");
    println!("  -Wall, --Wall        Enable common warnings (some advice remains opt-in)");
    println!("  -std=<revision>     Check implemented features: f90, f95, f2003, f2008, f2018, f2023");
    println!("                      Default: permissive; revision checks are partial");
    println!("  -Wunused             Enable all unused-declaration warnings");
    println!("  -Wunused-<category>  Enable type, variable, parameter, dummy-argument or function warnings");
    println!("  -Woptional-argument Warn about OPTIONAL dummy references without proven presence");
    println!("  -Wshort-circuit     Warn about unsafe ALLOCATED/ASSOCIATED logical guards");
    println!("  -Wimplicit-save     Warn about implicit SAVE from local declaration initialization");
    println!("  -Wmissing-intent    Warn about data dummy arguments without INTENT (opt-in)");
    println!("  -Wshadow            Warn about locals shadowing enclosing data declarations (opt-in)");
    println!("  -Wshadow-dummy-argument  Include dummy-argument shadowing (opt-in)");
    println!("  -Wdead-store        Warn about overwritten local scalar values (opt-in)");
    println!("  -Winteger-division  Warn about truncating constant integer divisions");
    println!("  -Wunchecked-status Warn about status results never inspected or forwarded (opt-in)");
    println!("  -Wtarget-lifetime   Warn about pointers escaping automatic targets");
    println!("  -Waliasing          Warn about conflicting overlapping actual arguments");
    println!("  -Wconversion        Warn about information-losing implicit numeric conversions (opt-in)");
    println!("  -Wprecision-loss    Warn about lower-precision work before widening");
    println!("  -Wpointer-init      Warn about pointer components without default initialization");
    println!("  -Wdo-subscript      Warn about loop-derived subscript ranges outside fixed bounds");
    println!("  -Wenum-exhaustive   Warn about SELECT CASE missing declared enumeration values");
    println!("  -Wimplicit-interface  Warn about calls lacking explicit interfaces (opt-in)");
    println!("  -Wintrinsic-shadow  Warn about user procedures shadowing intrinsics (opt-in)");
    println!("  -Wunderflow         Warn about nonzero constants rounding to zero");
    println!("  -Wempty-case        Warn about reversed constant CASE ranges");
    println!("  -Wunused-label      Warn about unreferenced statement labels");
    println!("  -Wno-<category>      Disable a warning category");
    println!("  -Werror, --Werror    Treat enabled warnings as errors");
    println!("  -Werror=<category>  Enable a warning category and treat it as an error");
    println!("  -ffixed-form         Read sources as fixed form (default for .f, .for, .ftn, .f77, .fpp)");
    println!("  -cpp                 Apply preprocessor #if/#ifdef/#else/#endif conditionals");
    println!("                      Default for .F, .FOR, .FPP, .F90 and other upper-case extensions");
    println!("  -nocpp               Do not apply preprocessor conditionals");
    println!("  -DNAME[=VALUE]       Define a preprocessor name for conditionals (VALUE defaults to 1)");
    println!("  -ffree-form          Read sources as free form, whatever their extension");
    println!("  -ffixed-line-length-<n>  Last column of fixed-form lines: a number or none (default 72)");
    println!("  -fd-lines-as-code    Compile fixed-form D lines instead of ignoring them");
    println!("  --lto                Enable link-time optimization");
    println!("  --opt-level <level>  Set optimization level (0, 1, 2, 3, s, z, S, Z)");
    println!("  --quiet              Suppress build progress (warnings remain visible)");
    println!("  --help               Show this help message");
    println!("  --debug-ir           Show IR before and after optimizations");
    println!("  --time               Report elapsed times on stderr");
    println!("  --run                Run the executable after building");
    println!("  -- <arguments>       Pass arguments to --run or the run subcommand");
    println!("  -q                   Suppress build progress (warnings remain visible)");
    println!("  -S, --emit-asm       Print generated assembly for the object file");
    println!("  -o <output>          Specify output file (default derived from last source)");
}

pub fn default_exe_output_path(input: &Path) -> PathBuf {
    let mut out = input.to_path_buf();
    if cfg!(target_os = "windows") {
        out.set_extension("exe");
    } else {
        if let Some(stem) = input.file_stem() {
            out = input.with_file_name(stem);
        }
    }
    out
}
