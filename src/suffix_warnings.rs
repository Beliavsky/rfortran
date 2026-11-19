// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! `-Wsource-suffix`, in -Wstyle: a source file named `.f95`, `.f03`, `.f08`,
//! `.f18` or `.f23` (or the upper-case forms, which are preprocessed). The
//! suffix marks free source form, as `.f90` does, and does not select or
//! check a revision of the standard; `.f90` is the suffix every compiler and
//! build tool takes for free form, while other compilers do not all accept
//! the others (gfortran does not recognize `.f18` or `.f23`).
use crate::sema::SemaSettings;
use anyhow::{bail, Result};

const CATEGORY: &str = "source-suffix";

/// The revisions that free-form suffixes other than `.f90` suggest, with the
/// `-std=` value that checks conformance to each.
const SUFFIXES: &[(&str, &str, &str)] = &[
    ("f95", "Fortran 95", "f95"),
    ("f03", "Fortran 2003", "f2003"),
    ("f08", "Fortran 2008", "f2008"),
    ("f18", "Fortran 2018", "f2018"),
    ("f23", "Fortran 2023", "f2023"),
];

/// The warning for a file name, if its suffix is one of SUFFIXES: the
/// message and its notes.
pub(crate) fn advice(path: &std::path::Path) -> Option<(String, Vec<String>)> {
    let extension = path.extension()?.to_str()?;
    let &(_, revision, std) = SUFFIXES.iter().find(|(s, _, _)| s.eq_ignore_ascii_case(extension))?;
    // Upper case marks a preprocessed source, as `.F90` does.
    let preprocessed = extension.chars().next().is_some_and(|c| c.is_ascii_uppercase());
    let free = if preprocessed { "F90" } else { "f90" };
    let renamed = path.with_extension(free);
    let renamed = renamed.file_name().and_then(|n| n.to_str()).unwrap_or(free);
    let mut notes = vec![format!("rename the file to `{renamed}`, and use -std={std} to check conformance to {revision}")];
    if matches!(std, "f2018" | "f2023") {
        notes.push(format!("gfortran does not recognize `.{extension}` as a Fortran source, and passes the file to the linker"));
    } else {
        notes.push(format!("some compilers, such as Intel's, take only `.{free}` for free source form without further options"));
    }
    let message = format!("suffix `.{extension}` does not select a Fortran standard: it marks free source form, as `.{free}` does for every compiler [{CATEGORY}]");
    Some((message, notes))
}

pub(crate) fn check(input: &std::path::Path, filename: &str, settings: &SemaSettings, options: &[String]) -> Result<()> {
    // In -Wstyle, not -Wall.
    let mut style_only = settings.clone();
    style_only.wall = false;
    let (enabled, error) = crate::unused::selected(CATEGORY, false, &style_only, options);
    if !enabled || settings.allow.contains(CATEGORY) { return Ok(()); }
    let Some((message, notes)) = advice(input) else { return Ok(()) };
    use codespan_reporting::{diagnostic::Diagnostic, files::SimpleFile,
        term::{emit_to_io_write, termcolor::{ColorChoice, StandardStream}, Config}};
    let d = if error { Diagnostic::error() } else { Diagnostic::warning() }
        .with_message(format!("{filename}: {message}"))
        .with_notes(notes);
    emit_to_io_write(&mut StandardStream::stderr(ColorChoice::Auto), &Config::default(), &SimpleFile::new(filename, ""), &d)?;
    if error { bail!("{CATEGORY} warnings treated as errors"); }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::advice;
    use std::path::Path;

    #[test]
    fn suffixes() {
        assert!(advice(Path::new("a.f90")).is_none());
        assert!(advice(Path::new("a.F90")).is_none());
        assert!(advice(Path::new("a.f")).is_none());
        let (message, notes) = advice(Path::new("dir/prog.f08")).unwrap();
        assert!(message.contains("`.f08`"), "{message}");
        assert!(notes[0].contains("`prog.f90`") && notes[0].contains("-std=f2008"), "{notes:?}");
        let (_, notes) = advice(Path::new("prog.F18")).unwrap();
        assert!(notes[0].contains("`prog.F90`") && notes[0].contains("-std=f2018"), "{notes:?}");
        assert!(notes[1].contains("gfortran"), "{notes:?}");
    }
}
