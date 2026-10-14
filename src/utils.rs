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
    println!("  link <inputs>        Link object files or sources into an executable");
    println!("\nOptions:");
    println!("  --verbose            Show build progress and toolchain commands");
    println!("  -c, --compile-only   Emit an object without linking");
    println!("  -Wall, --Wall        Enable all warnings");
    println!("  -Wunused             Enable all unused-declaration warnings");
    println!("  -Wunused-<category>  Enable type, variable, parameter, dummy-argument or function warnings");
    println!("  -Woptional-argument Warn about OPTIONAL dummy references without proven presence");
    println!("  -Wno-<category>      Disable a warning category");
    println!("  -Werror, --Werror    Treat enabled warnings as errors");
    println!("  -Werror=<category>  Enable a warning category and treat it as an error");
    println!("  --lto                Enable link-time optimization");
    println!("  --opt-level <level>  Set optimization level (0, 1, 2, 3, s, z, S, Z)");
    println!("  --quiet              Suppress build progress (warnings remain visible)");
    println!("  --help               Show this help message");
    println!("  --debug-ir           Show IR before and after optimizations");
    println!("  --run                Run the executable after building");
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
