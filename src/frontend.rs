// SPDX-License-Identifier: MIT
//! Source preparation and lowering shared by native and browser interpreters.
use crate::*;
use anyhow::Result;

pub(crate) fn prepare_program(
    program: ast::Program,
    src: &str,
    tokens: &[lexer::Token],
    input: &std::path::Path,
    origins: Option<&[source_diagnostics::SourceOrigin]>,
    settings: &sema::SemaSettings,
    args: &cli::Cli,
) -> Result<module_files::Prepared> {
    let filename = input.to_str().unwrap_or("<unknown>");
    unused::validate_options(&args.warnings)?;
    literal_range::check(tokens)
        .map_err(|error| source_diagnostics::report(error, src, tokens, filename, origins))?;
    optional_warnings::check(&program, src, tokens, filename, origins, settings, &args.warnings)?;
    declaration_warnings::check(&program, src, tokens, filename, origins, settings, &args.warnings)?;
    let prepared = module_files::prepare(program, src, input, origins, args)
        .map_err(|error| source_diagnostics::report(error, src, tokens, filename, origins))?;
    unused::check(&prepared.program, &prepared.source, &lexer::lex(&prepared.source), src.len(),
                  filename, Some(&prepared.origins), settings, &args.warnings)?;
    implicit_warnings::check(&prepared.program, &prepared.source, src.len(), filename,
                             &prepared.origins, settings, &args.warnings)?;
    short_circuit_warnings::check(&prepared.program, &prepared.source, src.len(), filename,
                                 &prepared.origins, settings, &args.warnings)?;
    source_advice::check(&prepared.program, &prepared.source, src.len(), filename,
                        &prepared.origins, settings, &args.warnings)?;
    reachability::check(&prepared.program, &prepared.source, src.len(), filename,
                        &prepared.origins, settings, &args.warnings)?;
    format_warnings::check(&prepared.program, &prepared.source, src.len(), filename,
                           &prepared.origins, settings, &args.warnings, args.standard)?;
    open_warnings::check(&prepared.source, src.len(), filename, &prepared.origins, settings, &args.warnings, args.standard)?;
    io_warnings::check(&prepared.source, src.len(), filename, &prepared.origins, settings, &args.warnings, args.standard)?;
    leak_warnings::check(&prepared.program, &prepared.source, src.len(), filename,
                         &prepared.origins, settings, &args.warnings)?;
    allocation_warnings::check(&prepared.program, &prepared.source, src.len(), filename,
                               &prepared.origins, settings, &args.warnings)?;
    action_warnings::check(&prepared.program, &prepared.source, src.len(), filename,
                           &prepared.origins, settings, &args.warnings)?;
    use_warnings::check(&prepared.program, &prepared.source, src.len(), filename,
                        &prepared.origins, settings, &args.warnings)?;
    parameter_warnings::check(&prepared.program, &prepared.source, src.len(), filename,
                              &prepared.origins, settings, &args.warnings)?;
    kind_warnings::check(&prepared.source, src.len(), filename, &prepared.origins, settings, &args.warnings)?;
    indentation_warnings::check(&prepared.source, src.len(), filename, &prepared.origins, settings, &args.warnings,
                                args.is_fixed_form(input))?;
    return_warnings::check(&prepared.source, src.len(), filename, &prepared.origins, settings, &args.warnings)?;
    style_warnings::check(&prepared.source, src.len(), filename, &prepared.origins, settings, &args.warnings,
                          args.is_fixed_form(input))?;
    if args.standard.is_some() {
        standards::check(&prepared.program, &prepared.source, &lexer::lex(&prepared.source), filename,
                         Some(&prepared.origins), args.standard, settings.werror)?;
    }
    Ok(prepared)
}

pub(crate) fn lower_program_body(
    program: ast::Program, src: &str, tokens: &[lexer::Token], filename: &str,
    origins: Option<&[source_diagnostics::SourceOrigin]>,
) -> Result<ast::Program> {
    (|| {
        do_variables::check(&program, src)?;
        let program = host_storage::resolve(program)?;
        let program = protected::resolve(program)?;
        let program = pdt::resolve(program)?;
        let program = character_pointers::resolve(program)?;
        let program = recursive_types::resolve(program)?;
        let program = recursive_outputs::resolve(program)?;
        let program = scalar_components::resolve(program)?;
        let program = multidim::shape_arguments(program);
        let program = common::resolve(program)?;
        let program = implicit_warnings::declare(program);
        let program = namelists::resolve(program)?;
        let program = c_pointers::prepare(program)?;
        let program = c_interop::prepare(program)?;
        elemental::reject_allocatable_results(&program)?;
        let program = compile_profile::run("storage_size::resolve", || storage_size::resolve(program))?;
        let program = compile_profile::run("statement_functions::resolve", || statement_functions::resolve(program))?;
        let program = compile_profile::run("transfer::resolve", || transfer::resolve(program))?;
        let program = compile_profile::run("defining_functions::resolve", || defining_functions::resolve(program))?;
        let program = compile_profile::run("equivalence::resolve", || equivalence::resolve(program))?;
        let program = compile_profile::run("external_calls::resolve", || external_calls::resolve(program))?;
        let program = compile_profile::run("derived::resolve", || derived::resolve(program))?;
        let program = compile_profile::run("defining_functions::resolve_components", || defining_functions::resolve_components(program))?;
        let program = compile_profile::run("external_calls::resolve_complex", || external_calls::resolve_complex(program))?;
        let program = compile_profile::run("random_seed::resolve", || random_seed::resolve(program))?;
        let program = compile_profile::run("parameters::resolve", || parameters::resolve(program))?;
        let program = c_interop::resolve(program)?;
        let program = compile_profile::run("timing_intrinsics::resolve", || timing_intrinsics::resolve(program))?;
        let program = compile_profile::run("tokenize::resolve", || tokenize::resolve(program))?;
        let program = compile_profile::run("array_calls::resolve", || array_calls::resolve(program))?;
        let program = compile_profile::run("reductions::resolve", || reductions::resolve(program))?;
        let program = compile_profile::run("sections::resolve", || sections::resolve(program))?;
        // RFORT_DUMP_LOWERED=1 prints the program after source-level lowering.
        if std::env::var_os("RFORT_DUMP_LOWERED").is_some_and(|v| v == "1") {
            eprintln!("{program:#?}");
        }
        Ok(program)
    })()
    .map_err(|error| source_diagnostics::report(error, src, tokens, filename, origins))
}

pub(crate) fn suppress_legacy_unused(settings: &mut sema::SemaSettings) {
    for category in ["unused_param", "unused_local", "unused_program_var", "unused_function"] {
        settings.allow.insert(category.into());
    }
}
