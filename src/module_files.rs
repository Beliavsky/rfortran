// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! Persistent, compiler-specific module definitions. Procedure bodies are kept
//! because the current compiler specializes array/CHARACTER/derived interfaces.
use crate::{
    ast::{Program, Stmt},
    cli::Cli,
    source_diagnostics::SourceOrigin,
};
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
};

const FORMAT: u32 = 1;
#[derive(Clone, Serialize, Deserialize)]
struct Interface {
    rfort_module_format: u32,
    compiler: String,
    target: String,
    name: String,
    fingerprint: String,
    dependencies: BTreeMap<String, String>,
    definition: Stmt,
    source: String,
    filename: String,
    origins: Vec<SourceOrigin>,
}

fn children(statement: &mut Stmt, visit: &mut impl FnMut(&mut Vec<Stmt>)) {
    match statement {
        Stmt::Module { body, .. }
        | Stmt::Function { body, .. }
        | Stmt::Subroutine { body, .. }
        | Stmt::Block { body }
        | Stmt::Do { body, .. }
        | Stmt::DoWhile { body, .. }
        | Stmt::DerivedType {
            components: body, ..
        } => visit(body),
        Stmt::If {
            then_body,
            else_body,
            ..
        } => {
            visit(then_body);
            if let Some(body) = else_body {
                visit(body);
            }
        }
        Stmt::SelectCase { cases, default, .. } => {
            for case in cases {
                visit(&mut case.body);
            }
            if let Some(body) = default {
                visit(body);
            }
        }
        _ => {}
    }
}
fn dependencies(definition: &Stmt) -> Vec<(String, bool)> {
    fn visit(body: &mut Vec<Stmt>, names: &mut BTreeMap<String, bool>) {
        for statement in body {
            if let Stmt::Use { module, nature, .. } = statement {
                if nature.as_deref() != Some("intrinsic") {
                    let required = nature.as_deref() == Some("non_intrinsic");
                    names
                        .entry(module.clone())
                        .and_modify(|v| *v |= required)
                        .or_insert(required);
                }
            }
            children(statement, &mut |body| visit(body, names));
        }
    }
    let mut names = BTreeMap::new();
    visit(&mut vec![definition.clone()], &mut names);
    names.into_iter().collect()
}
/// The source offset of the first USE of each module, for diagnostics;
/// `at` locates the statement before `definition`.
fn use_locations(definition: &Stmt, mut at: Option<usize>) -> HashMap<String, usize> {
    fn visit(body: &mut Vec<Stmt>, at: &mut Option<usize>, found: &mut HashMap<String, usize>) {
        for statement in body {
            match statement {
                Stmt::SourceLocation(p) => *at = Some(*p),
                Stmt::Use { module, .. } => {
                    if let Some(p) = *at {
                        found.entry(module.clone()).or_insert(p);
                    }
                }
                _ => {}
            }
            children(statement, &mut |body| visit(body, at, found));
        }
    }
    let mut found = HashMap::new();
    visit(&mut vec![definition.clone()], &mut at, &mut found);
    found
}
fn strip_locations(body: &mut Vec<Stmt>) {
    body.retain(|s| !matches!(s, Stmt::SourceLocation(_)));
    for statement in body {
        children(statement, &mut strip_locations);
    }
}
fn relocate(body: &mut Vec<Stmt>, offset: usize) {
    for statement in body {
        if let Stmt::SourceLocation(pos) = statement {
            *pos += offset;
        }
        children(statement, &mut |body| relocate(body, offset));
    }
}
fn fingerprint(definition: &Stmt, dependencies: &BTreeMap<String, String>) -> Result<String> {
    let mut body = vec![definition.clone()];
    strip_locations(&mut body);
    // A stable content identifier, not a security checksum. Includes private
    // bodies and transitive interface identities used during specialization.
    let bytes = serde_json::to_vec(&(body, dependencies))?;
    let hash = bytes.iter().fold(0xcbf29ce484222325u64, |h, b| {
        (h ^ u64::from(*b)).wrapping_mul(0x100000001b3)
    });
    Ok(format!("{hash:016x}"))
}
fn mark_saved(statement: &mut Stmt, scope: &str, ordinal: &mut usize) {
    let next = match statement {
        Stmt::Module { name, .. } => format!("@module${name}"),
        Stmt::Function { name, .. } | Stmt::Subroutine { name, .. } => format!("{scope}${name}"),
        _ => scope.to_owned(),
    };
    if let Stmt::InitializedDecl { attributes, .. } = statement {
        if scope.matches('$').count() >= 2
            && !attributes
                .iter()
                .any(|a| a == "parameter" || a.starts_with("@save_key:"))
        {
            *ordinal += 1;
            attributes.push(format!("@save_key:{scope}$decl{ordinal}"));
        }
    }
    children(statement, &mut |body| {
        for statement in body {
            mark_saved(statement, &next, ordinal);
        }
    });
}

pub(crate) struct Prepared {
    pub program: Program,
    pub source: String,
    pub origins: Vec<SourceOrigin>,
    pub owned: HashSet<String>,
    pending: Vec<Interface>,
    output: PathBuf,
}
struct Loader {
    search: Vec<PathBuf>,
    own: HashMap<String, Interface>,
    ready: HashMap<String, Interface>,
    visiting: HashSet<String>,
    order: Vec<String>,
}
impl Loader {
    fn load(&mut self, name: &str, required: bool) -> Result<Option<String>> {
        if let Some(interface) = self.ready.get(name) {
            return Ok(Some(interface.fingerprint.clone()));
        }
        if !self.visiting.insert(name.to_owned()) {
            bail!("circular module dependency involving `{name}`");
        }
        let mut interface = if let Some(interface) = self.own.get(name) {
            interface.clone()
        } else {
            let path = self
                .search
                .iter()
                .map(|p| p.join(format!("{name}.rmod")))
                .find(|p| p.is_file());
            let Some(path) = path else {
                self.visiting.remove(name);
                if matches!(name, "iso_fortran_env" | "ieee_arithmetic" | "iso_c_binding") && !required {
                    return Ok(None);
                }
                if !required && matches!(name, "iso_c_binding" | "ieee_arithmetic" | "ieee_exceptions" | "ieee_features") {
                    bail!("intrinsic module `{name}` is not yet supported");
                }
                bail!("module interface `{name}.rmod` not found; compile its module first or supply -I <directory>");
            };
            let interface: Interface =
                serde_json::from_slice(&fs::read(&path)?).with_context(|| {
                    format!(
                        "reading module interface {}; rebuild its module",
                        path.display()
                    )
                })?;
            if interface.rfort_module_format != FORMAT
                || interface.compiler != env!("CARGO_PKG_VERSION")
                || interface.target != target_lexicon::Triple::host().to_string()
            {
                bail!("incompatible module interface {}; rebuild its module with this compiler and target", path.display());
            }
            if interface.name != name
                || !matches!(&interface.definition, Stmt::Module {name: n, ..} if n == name)
                || interface.fingerprint
                    != fingerprint(&interface.definition, &interface.dependencies)?
            {
                bail!(
                    "invalid module interface {}; rebuild its module",
                    path.display()
                );
            }
            interface
        };
        let is_own = self.own.contains_key(name);
        let mut resolved = BTreeMap::new();
        // Only a module of this source group has offsets into its source.
        let locations = if is_own { use_locations(&interface.definition, None) } else { HashMap::new() };
        for (dependency, required) in dependencies(&interface.definition) {
            if let Some(identity) = self
                .load(&dependency, required)
                .with_context(|| format!("required by module `{name}`"))
                .map_err(|e| crate::source_diagnostics::at(e, locations.get(&dependency).copied()))?
            {
                resolved.insert(dependency, identity);
            }
        }
        if !is_own && resolved != interface.dependencies {
            bail!("stale module interface `{name}.rmod`: a dependency has changed; recompile module `{name}`");
        }
        interface.dependencies = resolved;
        interface.fingerprint = fingerprint(&interface.definition, &interface.dependencies)?;
        let identity = interface.fingerprint.clone();
        self.ready.insert(name.to_owned(), interface);
        self.order.push(name.to_owned());
        self.visiting.remove(name);
        Ok(Some(identity))
    }
}

pub(crate) fn prepare(
    program: Program,
    source: &str,
    filename: &Path,
    origins: Option<&[SourceOrigin]>,
    args: &Cli,
) -> Result<Prepared> {
    let output = args
        .module_dir
        .clone()
        .unwrap_or_else(|| PathBuf::from("."));
    let mut search = vec![output.clone(), PathBuf::from(".")];
    search.extend(args.module_paths.clone());
    if let Some(parent) = filename.parent().filter(|p| !p.as_os_str().is_empty()) {
        search.push(parent.to_owned());
    }
    let original_origins = origins.map(|o| o.to_vec()).unwrap_or_else(|| {
        vec![SourceOrigin {
            name: filename.display().to_string(),
            range: 0..source.len(),
            source: None, offset: 0,
        }]
    });
    let mut own = HashMap::new();
    for statement in &program.body {
        if let Stmt::Module { name, .. } = statement {
            let interface = Interface {
                rfort_module_format: FORMAT,
                compiler: env!("CARGO_PKG_VERSION").into(),
                target: target_lexicon::Triple::host().to_string(),
                name: name.clone(),
                fingerprint: String::new(),
                dependencies: BTreeMap::new(),
                definition: statement.clone(),
                source: source.into(),
                filename: filename.display().to_string(),
                origins: original_origins.clone(),
            };
            if own.insert(name.clone(), interface).is_some() {
                bail!("duplicate module `{name}` in source group");
            }
        }
    }
    let owned = own.keys().cloned().collect::<HashSet<_>>();
    let mut loader = Loader {
        search,
        own,
        ready: HashMap::new(),
        visiting: HashSet::new(),
        order: vec![],
    };
    let mut at = None;
    for statement in &program.body {
        if let Stmt::SourceLocation(p) = statement {
            at = Some(*p);
        }
        if let Stmt::Module { name, .. } = statement {
            loader.load(name, true)?;
        } else {
            let locations = use_locations(statement, at);
            for (name, required) in dependencies(statement) {
                loader
                    .load(&name, required)
                    .map_err(|e| crate::source_diagnostics::at(e, locations.get(&name).copied()))?;
            }
        }
    }
    let mut expanded = program.clone();
    expanded.body.retain(|s| !matches!(s, Stmt::Module { .. }));
    let mut body = vec![];
    let mut combined_source = source.to_owned();
    let mut combined_origins = original_origins;
    let mut pending = vec![];
    for name in &loader.order {
        let interface = &loader.ready[name];
        let mut definition = vec![interface.definition.clone()];
        if owned.contains(name) {
            pending.push(interface.clone());
        } else {
            combined_source.push('\n');
            let offset = combined_source.len();
            combined_source.push_str(&interface.source);
            relocate(&mut definition, offset);
            for origin in &interface.origins {
                combined_origins.push(SourceOrigin {
                    name: origin.name.clone(),
                    range: origin.range.start + offset..origin.range.end + offset,
                    ..origin.clone()
                });
            }
        }
        mark_saved(&mut definition[0], "", &mut 0);
        body.extend(definition);
    }
    body.append(&mut expanded.body);
    expanded.body = body;
    Ok(Prepared {
        program: expanded,
        source: combined_source,
        origins: combined_origins,
        owned,
        pending,
        output,
    })
}
impl Prepared {
    /// Publish only after semantic checks and native code generation succeeded.
    pub fn publish(&self) -> Result<()> {
        if self.pending.is_empty() {
            return Ok(());
        }
        fs::create_dir_all(&self.output)
            .with_context(|| format!("creating module directory {}", self.output.display()))?;
        for interface in &self.pending {
            let path = self.output.join(format!("{}.rmod", interface.name));
            let bytes = serde_json::to_vec(interface)?;
            if fs::read(&path).ok().as_deref() == Some(bytes.as_slice()) {
                continue;
            }
            let temporary = self.output.join(format!(
                ".{}.{}.rmod.tmp",
                interface.name,
                uuid::Uuid::new_v4()
            ));
            let result = (|| {
                fs::write(&temporary, bytes)?;
                fs::rename(&temporary, &path)?;
                Ok::<_, std::io::Error>(())
            })();
            if result.is_err() {
                let _ = fs::remove_file(&temporary);
            }
            result.with_context(|| format!("writing module interface {}", path.display()))?;
        }
        Ok(())
    }
}
