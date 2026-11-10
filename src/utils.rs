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
pub(crate) fn read_source_unit(path: &Path, cli: &crate::cli::Cli, fixed: bool, preprocess: bool) -> Result<String> {
    // Preprocessor conditionals come first; their lines become blank.
    let preprocessed = if preprocess {
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
    println!("  -I <directory>       Search for INCLUDE files and .rmod module interfaces (repeatable)");
    println!("  -J <directory>       Write module interfaces here and search this directory");
    println!("  -Wall, --Wall        Enable common warnings (some advice remains opt-in)");
    println!("  -Wextra, --Wextra    Enable additional opt-in advice:");
    println!("                      missing-intent, shadow, shadow-dummy-argument, dead-store,");
    println!("                      unchecked-status, intrinsic-shadow, keyword-name, fixed-unit");
    println!("                      As in gfortran, it does not imply -Wall; use -Wall -Wextra");
    println!("  -std=<revision>     Check implemented features: f90, f95, f2003, f2008, f2018, f2023");
    println!("                      Default: permissive; revision checks are partial");
    println!("  -Wunused             Enable all unused-declaration warnings");
    println!("  -Wunused-<category>  Enable type, variable, parameter, dummy-argument or function warnings");
    println!("  -Woptional-argument Warn about OPTIONAL dummy references without proven presence");
    println!("  -Wshort-circuit     Warn about unsafe ALLOCATED/ASSOCIATED logical guards");
    println!("  -Wimplicit-typing   Warn about variables typed by implicit typing rules");
    println!("  -Wimplicit-save     Warn about implicit SAVE from local declaration initialization");
    println!("  --default-private   Default user module entities to PRIVATE; explicit accessibility wins");
    println!("  -Wmodule-default-public  Warn about modules without an explicit default PRIVATE or PUBLIC (opt-in)");
    println!("  -Wuse-without-only  Warn about USE without ONLY and suggest the ONLY list the scope needs (opt-in)");
    println!("  -Wcould-be-parameter  Suggest named constants for locals set once from a constant and never modified (opt-in)");
    println!("  -Wliteral-kind      Warn about kind numbers such as real(8), real*8 and 1.0_8, and suggest ISO_FORTRAN_ENV names (opt-in)");
    println!("  -Wdefault-real      Warn about REAL and COMPLEX declarations without a kind (opt-in)");
    println!("  -Wdimension-override Warn about entity shapes or lengths overriding DIMENSION or the type length (in -Wextra)");
    println!("  -Winteger-division-all Warn about every integer division whose quotient is truncated (opt-in)");
    println!("  -Windentation       Warn about unindented or inconsistently indented construct bodies and misaligned END, ELSE and CASE (opt-in)");
    println!("  -Wearly-return      Warn about RETURN statements after which statements of the procedure can still run (opt-in)");
    println!("  -Wcompare-reals     Warn about == and /= with REAL or COMPLEX operands (in -Wextra)");
    println!("  -Wlogical-comparison Warn about comparisons with .true. or .false. (in -Wextra and -Wstyle)");
    println!("  -Wstyle             Enable the style warnings after Clerman and Spector, listed below, with other style checks");
    println!("  -Wold-relational    Warn about .lt., .le., .eq., .ne., .gt. and .ge. (in -Wstyle)");
    println!("  -Wmissing-double-colon Warn about type declarations without :: (in -Wstyle)");
    println!("  -Wlabeled-do        Warn about DO loops ended by a labeled statement (in -Wstyle)");
    println!("  -Wdouble-precision  Warn about DOUBLE PRECISION and DOUBLE COMPLEX (in -Wstyle)");
    println!("  -Wbare-end          Warn about END statements without the unit or type name (in -Wstyle)");
    println!("  -Wkeyword-case      Warn about keywords written in more than one case (in -Wstyle)");
    println!("  -Wtabs              Warn about tab characters in free-form code (in -Wstyle)");
    println!("  -Wline-length[=N]   Warn about free-form lines longer than 132 or N characters (in -Wstyle)");
    println!("  -Wduplicate-constant Warn about REAL named constants and kind parameters defined in several scopes (in -Wstyle)");
    println!("  -Wdeclaration-order Warn about dummies declared after locals or the result (in -Wstyle)");
    println!("  -Wargument-order    Warn about INTENT(IN) arguments after outputs, or required after OPTIONAL (in -Wstyle)");
    println!("  -Wsemicolon         Warn about lines with more than one statement, except CASE branches (in -Wstyle)");
    println!("  -Wtrailing-whitespace Warn about free-form lines ending in spaces or tabs (in -Wstyle)");
    println!("  -Wunnamed-exit      Warn about EXIT and CYCLE without a construct name (in -Wstyle)");
    println!("  -Wgoto              Warn about GO TO statements (in -Wstyle)");
    println!("  -Wobsolescent       Warn about obsolescent and deleted features such as COMMON and computed GO TO (in -Wstyle)");
    println!("  -Wstop-in-procedure Warn about STOP in functions and subroutines (in -Wstyle)");
    println!("  -Wassumed-size      Warn about assumed-size dummy arrays (in -Wstyle)");
    println!("  -Wredundant-implicit Warn about IMPLICIT NONE already in effect from the host (in -Wstyle)");
    println!("  -Wname-case         Warn about names written in a case different from their declaration (in -Wstyle)");
    println!("  -Wfunction-result   Warn about FUNCTION statements with a prefix type or without RESULT (in -Wstyle)");
    println!("  -Wcould-be-conditional Suggest Fortran 2023 conditional expressions for IF blocks that only assign one variable (opt-in)");
    println!("  -Wcould-be-one-line-if Suggest one-line IFs for IF blocks holding a single statement (opt-in)");
    println!("  -Wone-line-if       Warn about one-line IFs, suggesting the block form (opt-in)");
    println!("  -Wloop-runs-once    Warn about DO loops whose body cannot start a second iteration (in -Wall)");
    println!("  -Wreal-do-variable  Warn about DO loops with a REAL variable (in -Wall)");
    println!("  -Wintent-out-unset  Warn about INTENT(OUT) dummy arguments not assigned on every path (in -Wextra)");
    println!("  -Wcould-be-intent-out Warn about INTENT(INOUT) scalars assigned before any read (in -Wextra)");
    println!("  -Wredundant-cycle   Warn about CYCLE at the end of a loop body (in -Wextra)");
    println!("  -Wmaybe-uninitialized Warn about local scalars that may be read before being assigned (in -Wextra)");
    println!("  -Wunchecked-conformance Warn about array dummies combined without checking their sizes (in -Wextra)");
    println!("  -Wconformance       Warn about calls whose array sizes break a procedure's requirements (in -Wall)");
    println!("  -Wmaybe-nonconforming Warn about operations on arrays whose sizes from PACK or ALLOCATE may differ (in -Wextra)");
    println!("  -Wmaybe-out-of-bounds Warn about constant subscripts of PACK results (in -Wextra)");
    println!("  -Wcould-be-intent-in Warn about INTENT(INOUT) dummy arguments that are never modified (in -Wextra)");
    println!("  -Wunused-only       Warn about USE ONLY names that are never used (in -Wextra)");
    println!("  -Winfinite-loop     Warn about DO loops without loop control that nothing in their body can end (enabled by -Wall)");
    println!("  -Wmissing-intent    Warn about data dummy arguments without INTENT (opt-in)");
    println!("  -Wshadow            Warn about locals shadowing enclosing data declarations (opt-in)");
    println!("  -Wshadow-dummy-argument  Include dummy-argument shadowing (opt-in)");
    println!("  -Wdead-store        Warn about overwritten local scalar values (opt-in)");
    println!("  -Winteger-division  Warn about truncating constant integer divisions");
    println!("  -Wunchecked-status Warn about status results never inspected or forwarded (opt-in)");
    println!("  -Wtarget-lifetime   Warn about pointers escaping automatic targets");
    println!("  -Waliasing          Warn about conflicting overlapping actual arguments");
    println!("  -Wformat-reversion  Warn about lists with more items than the format's descriptors (in -Wextra)");
    println!("  -Wformat-unused     Warn about format descriptors that no list item reaches (in -Wextra)");
    println!("  -Wcould-be-at       Suggest the AT edit descriptor in place of trim (in -Wstyle with --std=f2023)");
    println!("  -Wconversion        Warn about information-losing implicit numeric conversions (opt-in)");
    println!("  -Wcmplx-kind        Warn about wider REAL arguments to CMPLX without KIND (included in -Wall and -Wconversion)");
    println!("  -Wprecision-loss    Warn about lower-precision work before widening");
    println!("  -Wreal-literal-kind Warn about real literals without an explicit kind (opt-in)");
    println!("  -Wexcess-real-digits Warn about more decimal digits than a real kind guarantees");
    println!("  -Wpointer-init      Warn about pointer components without default initialization");
    println!("  -Wdo-subscript      Warn about loop-derived subscript ranges outside fixed bounds");
    println!("  -Wenum-exhaustive   Warn about SELECT CASE missing declared enumeration values");
    println!("  -Wimplicit-interface  Warn about calls lacking explicit interfaces (opt-in)");
    println!("  -Wintrinsic-shadow  Warn about user procedures shadowing intrinsics (opt-in)");
    println!("  -Wunderflow         Warn about nonzero constants rounding to zero");
    println!("  -Wempty-case        Warn about reversed constant CASE ranges");
    println!("  -Wunused-label      Warn about unreferenced statement labels");
    println!("  -Wcharacter-truncation  Warn about CHARACTER values of known length truncated by assignment");
    println!("  -Wimplicit-open     Warn about READ/WRITE on a constant unit no OPEN names (on by default;");
    println!("                      the unit connects to fort.N, as in gfortran and ifx)");
    println!("  -Wmemory-leak       Warn about local POINTER storage lost without DEALLOCATE (on by default)");
    println!("  -Wallocation-state  Warn about DEALLOCATE of an unallocated or ALLOCATE of an allocated local (on by default)");
    println!("  -Waction-mismatch   Warn about READ from a unit opened for WRITE or WRITE to one opened for READ (on by default)");
    println!("  -Wfixed-unit        Warn about OPEN of a constant unit in a procedure; suggest NEWUNIT= (opt-in)");
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
    println!("  -nocheck             Compile without run-time checks of uninitialized reads, subscripts and integer overflow");
    println!("  -fast                Same as -O3 -nocheck");
    println!("  --quiet              Suppress build progress (warnings remain visible)");
    println!("  --help               Show this help message");
    println!("  --debug-ir           Show IR before and after optimizations");
    println!("  --parallel           Enable experimental native DO CONCURRENT parallelism");
    println!("  --parallel-report    Explain parallel workers and serial fallback");
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
