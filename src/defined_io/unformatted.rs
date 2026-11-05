// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
//! Defined unformatted stream transfers. Parent and child operations share the
//! file position; no formatted session or record terminator is introduced.
use super::*;
use crate::operators::{ExpressionElement, ExpressionTypes};

fn transfer(s: &Stmt) -> Option<(bool, &[Expr])> {
    match s {
        Stmt::CallSub { name, args } if name == "@unformatted_read" => Some((true, args)),
        Stmt::CallSub { name, args } if name == "@unformatted_write" => Some((false, args)),
        _ => None,
    }
}

struct Lower<'a> {
    readers: HashMap<String, String>,
    writers: HashMap<String, String>,
    reader_components: HashSet<String>,
    writer_components: HashSet<String>,
    types: &'a ExpressionTypes,
    serial: usize,
}
impl Lower<'_> {
    fn item(
        &mut self,
        item: Expr,
        input: bool,
        unit: &str,
        ios: &str,
        msg: &str,
        objects: &Objects,
        declarations: &Declarations,
    ) -> Result<Vec<Stmt>> {
        if let Expr::Call(n, a) = &item {
            if matches!(n.as_str(), "@input_implied_do" | "@output_implied_do") && a.len() > 4 {
                let mut body = vec![call("@do_step", a[..4].to_vec())];
                body.push(input::nonzero(ios, vec![Stmt::Exit]));
                for e in &a[4..] {
                    body.extend(self.item(
                        e.clone(),
                        input,
                        unit,
                        ios,
                        msg,
                        objects,
                        declarations,
                    )?);
                }
                return Ok(vec![when_equal(ios, 0, vec![Stmt::Block { body }], None)]);
            }
        }
        let kind = self.types.type_rank(&item, declarations, objects)?;
        let binding = match &kind {
            Some((ExpressionElement::Derived(ty), _)) => {
                if input { &self.readers } else { &self.writers }
                    .get(ty)
                    .cloned()
            }
            _ => None,
        };
        if let Some(binding) = binding {
            self.serial += 1;
            let prefix = format!("@defined_binary_item_{}", self.serial);
            let indexed = matches!(&item, Expr::Call(n,_) | Expr::Index(n,_) if
                matches!(self.types.type_rank(&id(n), declarations, objects)?, Some((_, r)) if r > 0));
            let (value, mut before, loops) = if kind.is_some_and(|(_, rank)| rank > 0) || indexed {
                array_item(item, &prefix, self.types, declarations, objects)?
            } else {
                if input && !matches!(item, Expr::Ident(_)) {
                    bail!("defined unformatted input requires writable variables, elements or sections");
                }
                (item, vec![], vec![])
            };
            let mut body = vec![
                crate::imports::reference(binding.clone()),
                call(&binding, vec![value, id(unit), id(ios), id(msg)]),
            ];
            for (index, first, last, step) in loops {
                let mut inner = vec![
                    call("@do_step", vec![id(&index), first, last, step]),
                    input::nonzero(ios, vec![Stmt::Exit]),
                ];
                inner.extend(body);
                body = vec![Stmt::Block { body: inner }];
            }
            before.extend(body);
            Ok(vec![when_equal(
                ios,
                0,
                vec![Stmt::Block { body: before }],
                None,
            )])
        } else {
            // Ordinary intrinsic/default derived transfers retain the existing
            // byte layout and array lowering, but report failure to the parent.
            let raw = call(
                if input {
                    "@unformatted_read"
                } else {
                    "@unformatted_write"
                },
                vec![id(unit), Expr::Str("iostat".into()), id(ios), int(0), item],
            );
            Ok(vec![when_equal(
                ios,
                0,
                vec![
                    raw,
                    input::nonzero(
                        ios,
                        vec![assign(
                            msg,
                            Expr::Str("unformatted transfer failed: end of file or error".into()),
                        )],
                    ),
                ],
                None,
            )])
        }
    }

    fn one(
        &mut self,
        s: &Stmt,
        message: Option<&Expr>,
        objects: &Objects,
        declarations: &Declarations,
    ) -> Result<Option<Stmt>> {
        let Some((input, args)) = transfer(s) else {
            return Ok(None);
        };
        let [unit, Expr::Str(control), status, position, items @ ..] = args else {
            bail!("invalid unformatted transfer");
        };
        // Recurse through implied-DO lists when deciding whether dispatch is needed.
        fn has_binding(
            item: &Expr,
            types: &ExpressionTypes,
            declarations: &Declarations,
            objects: &Objects,
            bindings: &HashMap<String, String>,
            component_bindings: &HashSet<String>,
        ) -> Result<bool> {
            if let Expr::Call(n, a) = item {
                if matches!(n.as_str(), "@input_implied_do" | "@output_implied_do") && a.len() > 4 {
                    for e in &a[4..] {
                        if has_binding(
                            e,
                            types,
                            declarations,
                            objects,
                            bindings,
                            component_bindings,
                        )? {
                            return Ok(true);
                        }
                    }
                    return Ok(false);
                }
            }
            if let Some((ExpressionElement::Derived(ty), _)) =
                types.type_rank(item, declarations, objects)?
            {
                if bindings.contains_key(&ty) {
                    return Ok(true);
                }
                if component_bindings.contains(&ty) {
                    bail!("default unformatted transfer of `{ty}` with defined-I/O components is not yet supported; supply an unformatted binding for the enclosing type");
                }
            }
            Ok(false)
        }
        let bindings = if input { &self.readers } else { &self.writers };
        if bindings.is_empty() && message.is_none() {
            return Ok(None);
        }
        let mut needed = message.is_some();
        let component_bindings = if input {
            &self.reader_components
        } else {
            &self.writer_components
        };
        for item in items {
            needed |= has_binding(
                item,
                self.types,
                declarations,
                objects,
                bindings,
                component_bindings,
            )?;
        }
        if !needed {
            return Ok(None);
        }
        if !matches!(
            self.types.type_rank(unit, declarations, objects)?,
            Some((ExpressionElement::Intrinsic(TypeSpec::Integer(_)), 0))
        ) {
            bail!("defined unformatted I/O requires an external INTEGER unit");
        }
        if !control.is_empty()
            && (control != "iostat"
                || !matches!(status, Expr::Ident(_))
                || !matches!(
                    self.types.type_rank(status, declarations, objects)?,
                    Some((ExpressionElement::Intrinsic(TypeSpec::Integer(_)), 0))
                ))
        {
            bail!("defined unformatted IOSTAT requires a scalar INTEGER variable");
        }
        if let Some(message) = message {
            if !matches!(message, Expr::Ident(_))
                || !matches!(
                    self.types.type_rank(message, declarations, objects)?,
                    Some((
                        ExpressionElement::Intrinsic(
                            TypeSpec::Character(_)
                                | TypeSpec::CharacterLength(_)
                                | TypeSpec::CharacterAssumed
                        ),
                        0
                    ))
                )
            {
                bail!("defined unformatted IOMSG requires a scalar CHARACTER variable");
            }
        }
        self.serial += 1;
        let prefix = format!("@defined_binary_{}", self.serial);
        let (u, ios, msg) = (
            format!("{prefix}_unit"),
            format!("{prefix}_status"),
            format!("{prefix}_message"),
        );
        let mut body = vec![
            decl(&u, TypeSpec::Integer(None)),
            decl(&ios, TypeSpec::Integer(None)),
            decl(&msg, TypeSpec::Character(Some(1024))),
            assign(&u, unit.clone()),
            assign(&ios, int(0)),
            assign(&msg, Expr::Str(String::new())),
        ];
        if crate::parameters::integer_constant(position).ok() != Some(0) {
            body.push(call(
                "@stream_seek",
                vec![id(&ios), id(&u), position.clone()],
            ));
            body.push(input::nonzero(
                &ios,
                vec![assign(
                    &msg,
                    Expr::Str("unformatted positioning failed".into()),
                )],
            ));
        }
        for item in items {
            body.extend(self.item(item.clone(), input, &u, &ios, &msg, objects, declarations)?);
        }
        if let Some(Expr::Ident(n)) = message {
            body.push(input::nonzero(&ios, vec![assign(n, id(&msg))]));
        }
        if control.is_empty() {
            body.push(input::nonzero(
                &ios,
                vec![call("@error_stop", vec![id(&msg)])],
            ));
        } else if let Expr::Ident(n) = status {
            body.push(assign(n, id(&ios)));
        }
        Ok(Some(Stmt::Block { body }))
    }

    fn body(
        &mut self,
        body: &mut [Stmt],
        inherited: &Objects,
        inherited_declarations: &Declarations,
    ) -> Result<()> {
        let mut objects = inherited.clone();
        let mut declarations = inherited_declarations.clone();
        for s in body.iter() {
            match s {
                Stmt::DerivedDecl {
                    type_name,
                    names,
                    dims,
                    ..
                } => {
                    for n in names {
                        objects.insert(n.clone(), (type_name.clone(), dims.len()));
                        declarations.remove(n);
                    }
                }
                Stmt::VarDecl { kind, names } => {
                    for n in names {
                        declarations.insert(n.clone(), (kind.clone(), 0));
                        objects.remove(n);
                    }
                }
                Stmt::InitializedDecl { kind, entities, .. } => {
                    for (n, _) in entities {
                        declarations.insert(n.clone(), (kind.clone(), 0));
                        objects.remove(n);
                    }
                }
                Stmt::ArrayDecl { kind, name, dims } => {
                    declarations.insert(name.clone(), (kind.clone(), dims.len()));
                    objects.remove(name);
                }
                _ => {}
            }
        }
        let mut offset = None;
        for s in body {
            if let Stmt::SourceLocation(p) = s {
                offset = Some(*p);
            }
            let result = (|| -> Result<()> {
                let wrapped = match s {
                    Stmt::Block { body } => match body.as_slice() {
                        [read, Stmt::CallSub { name, args }]
                            if name == crate::initialized::MAY_DEFINE && args.len() == 1 =>
                        {
                            self.one(read, Some(&args[0]), &objects, &declarations)?
                        }
                        _ => None,
                    },
                    _ => None,
                };
                if let Some(new) = wrapped.or(self.one(s, None, &objects, &declarations)?) {
                    *s = new;
                } else {
                    for child in crate::procedure_arguments::children(s) {
                        self.body(child, &objects, &declarations)?;
                    }
                }
                Ok(())
            })();
            result.map_err(|e| crate::source_diagnostics::at(e, offset))?;
        }
        Ok(())
    }
}

pub(super) fn resolve(program: &mut Program) -> Result<()> {
    // Avoid cloning all procedure bodies in programs that need no defined
    // binary dispatch or unformatted IOMSG handling.
    fn needed(body: &mut [Stmt]) -> bool {
        body.iter_mut().any(|s| {
            matches!(s, Stmt::DerivedType { bindings, .. } if bindings.iter().any(|(n, _)|
                matches!(n.as_str(), "@defined_io:read:unformatted" | "@defined_io:write:unformatted")))
            || matches!(s, Stmt::Block { body } if matches!(body.as_slice(),
                [io, Stmt::CallSub { name, args }] if transfer(io).is_some()
                    && name == crate::initialized::MAY_DEFINE && args.len() == 1))
            || crate::procedure_arguments::children(s).into_iter().any(|b| needed(b))
        })
    }
    if !needed(&mut program.body) {
        return Ok(());
    }
    let mut maps = vec![];
    for input in [true, false] {
        let (mut bindings, mut procedures, mut owners) =
            (HashMap::new(), HashMap::new(), HashMap::new());
        collect_form(
            &mut program.body,
            &mut bindings,
            &mut procedures,
            &mut owners,
            input,
            false,
        )?;
        for (ty, target) in &bindings {
            let procedure = procedures
                .get(target)
                .with_context(|| format!("undefined defined I/O procedure `{target}`"))?;
            let offset = match procedure {
                Stmt::Subroutine { body, .. } | Stmt::Function { body, .. } => body
                    .iter()
                    .find_map(crate::source_diagnostics::statement_offset),
                _ => None,
            };
            validate_form(&owners[ty], target, procedure, input, false).map_err(|e| {
                crate::source_diagnostics::at(
                    anyhow::anyhow!(e.to_string().replace("formatted output", "unformatted I/O")),
                    offset,
                )
            })?;
        }
        maps.push(bindings);
    }
    fn has_transfer(body: &mut [Stmt]) -> bool {
        body.iter_mut().any(|s| {
            transfer(s).is_some()
                || crate::procedure_arguments::children(s)
                    .into_iter()
                    .any(|b| has_transfer(b))
        })
    }
    if !has_transfer(&mut program.body) {
        return Ok(());
    }
    let types = ExpressionTypes::new(program)?;
    let writers = maps.pop().unwrap();
    let readers = maps.pop().unwrap();
    // Do not silently flatten past a component's defined binding when its
    // enclosing type has no binding of its own.
    fn component_types(body: &mut [Stmt], types: &mut HashMap<String, Vec<String>>) {
        for s in body {
            if let Stmt::DerivedType {
                name, components, ..
            } = s
            {
                types.insert(
                    name.clone(),
                    components
                        .iter()
                        .filter_map(|c| match c {
                            Stmt::DerivedDecl { type_name, .. } => Some(type_name.clone()),
                            _ => None,
                        })
                        .collect(),
                );
            }
            for b in crate::procedure_arguments::children(s) {
                component_types(b, types);
            }
        }
    }
    fn affected(
        types: &HashMap<String, Vec<String>>,
        bindings: &HashMap<String, String>,
    ) -> HashSet<String> {
        let mut found: HashSet<_> = bindings.keys().cloned().collect();
        loop {
            let next: Vec<_> = types
                .iter()
                .filter(|(n, fields)| {
                    !found.contains(*n) && fields.iter().any(|f| found.contains(f))
                })
                .map(|(n, _)| n.clone())
                .collect();
            if next.is_empty() {
                return found;
            }
            found.extend(next);
        }
    }
    let mut components = HashMap::new();
    component_types(&mut program.body, &mut components);
    let reader_components = affected(&components, &readers);
    let writer_components = affected(&components, &writers);
    Lower {
        readers,
        writers,
        reader_components,
        writer_components,
        types: &types,
        serial: 0,
    }
    .body(&mut program.body, &Objects::new(), &Declarations::new())
}
