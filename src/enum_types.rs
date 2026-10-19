// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
//! Named interoperable ENUM types (F2023), distinct from ENUMERATION TYPE.
use super::*;

pub fn definition(
    name: String,
    bind: String,
    language: String,
    items: Vec<(usize, Vec<(String, Option<Expr>)>)>,
) -> std::result::Result<Vec<Stmt>, &'static str> {
    if bind != "bind" || language != "c" {
        return Err("an enum definition requires BIND(C)");
    }
    let mut seen = HashSet::from([name.clone()]);
    if items
        .iter()
        .flat_map(|(_, items)| items)
        .any(|(n, _)| !seen.insert(n.clone()))
    {
        return Err("enum type and enumerator names must be distinct");
    }
    let mut out = vec![Stmt::DerivedType {
        name: name.clone(),
        components: vec![],
        bindings: vec![("@enumeration".into(), format!("cenum:{}", items[0].0))],
    }];
    let mut previous = None;
    for (pos, items) in items {
        for (member, value) in items {
            let value = value.unwrap_or_else(|| {
                previous.as_ref().map_or_else(
                    || int(0),
                    |p: &String| {
                        Expr::Bin(
                            BinOp::Add,
                            Box::new(Expr::Ident(p.clone())),
                            Box::new(int(1)),
                        )
                    },
                )
            });
            out.push(Stmt::SourceLocation(pos));
            // Retain the owner type through module renaming and interface
            // serialization. Scope collection treats these as INTEGER constants;
            // preparation captures conformance before substituting their values.
            out.push(Stmt::DerivedDecl {
                type_name: name.clone(),
                names: vec![member.clone()],
                attributes: vec!["parameter".into(), "@enum_member".into()],
                polymorphic: false,
                dims: vec![],
            });
            out.push(Stmt::Assign {
                name: member.clone(),
                value,
            });
            previous = Some(member);
        }
    }
    Ok(out)
}

impl Context {
    /// INTEGER result inference shares the operator resolver and numeric model
    /// catalog. Actual enum operands have already been checked by expression().
    pub(super) fn intrinsic_type(
        &self,
        name: &str,
        args: &[Expr],
        types: &[Ty],
        scope: &Scope,
    ) -> Result<Ty> {
        let Some(inference) = &self.inference else {
            return Ok(Ty::Unknown);
        };
        if name.starts_with('@') {
            return Ok(Ty::Unknown);
        }
        let mut declarations = HashMap::new();
        for source in [&self.globals, scope] {
            for (n, kind) in &source.kinds {
                let rank = source
                    .shapes
                    .get(n)
                    .map(Vec::len)
                    .unwrap_or_else(|| self.name(n, scope).rank());
                declarations.insert(n.clone(), (kind.clone(), rank));
            }
        }
        let mut arguments = args.to_vec();
        // KIND selectors are specification expressions, not runtime reads.
        // Resolve them with this scope's constants before asking shared inference.
        let kind_slot = match name {
            "int" | "nint" | "floor" | "ceiling" | "len" | "len_trim" | "ichar" | "iachar" => {
                Some(1)
            }
            "index" | "scan" | "verify" => Some(3),
            _ => None,
        };
        if let Some(arg) = kind_slot.and_then(|i| arguments.get_mut(i)) {
            if let Some(value) = self.constant(arg, scope) {
                *arg = int(value);
            }
        }
        let e = Expr::Call(name.into(), arguments);
        if let Some((kind, rank)) = inference.kind_rank(&e, &declarations)? {
            return Ok(Self::scalar(&kind, rank));
        }
        // The bit-intrinsic catalog also supplies kinds and argument checking.
        // These functions are elemental, so INTEGER array ranks are retained.
        if crate::bit_intrinsics::keys(name).is_some() || types.iter().all(|ty| ty.rank() == 0) {
            let kind = crate::inquiries::model_type(&e, &|n| declarations.get(n).cloned())?;
            if let Some(kind) = kind {
                return Ok(Self::scalar(
                    &kind,
                    types.iter().map(Ty::rank).max().unwrap_or(0),
                ));
            }
        }
        Ok(Ty::Unknown)
    }

    pub(super) fn c_enum_items(
        &self,
        items: &mut [Expr],
        explicit: Option<&str>,
        scope: &Scope,
        prepare: bool,
    ) -> Result<Option<Ty>> {
        let types = items
            .iter()
            .map(|e| self.expression(&mut e.clone(), scope, false))
            .collect::<Result<Vec<_>>>()?;
        let name = explicit.map(str::to_owned).or_else(|| {
            types
                .iter()
                .filter_map(Ty::enumeration)
                .find(|n| self.is_c_enum(n))
                .map(str::to_owned)
        });
        let Some(name) = name else {
            return Ok(None);
        };
        for (item, ty) in items.iter_mut().zip(types) {
            if ty.rank() > 1 {
                bail!("array constructor elements must be scalar or rank one");
            }
            self.check_value(&Ty::Enum(name.clone(), ty.rank()), item, scope, prepare)
                .map_err(|e| anyhow::anyhow!("enum array constructor element: {e}"))?;
        }
        Ok(Some(Ty::Enum(name, 1)))
    }

    pub(super) fn c_enum_array_helper(&mut self, ty: &str) -> String {
        self.serial += 1;
        let name = format!("@enum_array_conversion_{}", self.serial);
        let mut failed = vec![];
        if let Some(offset) = self.location {
            failed.push(Stmt::SourceLocation(offset));
        }
        failed.push(Stmt::CallSub {
            name: "@error_stop".into(),
            args: vec![Expr::Str(format!(
                "@enumeration_runtime:enum `{ty}` integer value outside the supported C int range"
            ))],
        });
        self.helpers.push(Stmt::Function {
            name: name.clone(),
            params: vec!["value".into()],
            return_type: Some(TypeSpec::Integer(None)),
            pure: true,
            elemental: true,
            body: vec![
                Stmt::VarDecl {
                    kind: TypeSpec::Integer(Some(8)),
                    names: vec!["value".into()],
                },
                Stmt::VarDecl {
                    kind: TypeSpec::Integer(None),
                    names: vec!["converted".into()],
                },
                Stmt::DummyIntent {
                    name: "value".into(),
                    intent: "intent(in)".into(),
                },
                Stmt::If {
                    cond: Expr::or(
                        Expr::lt(Expr::Ident("value".into()), int(i32::MIN as i64)),
                        Expr::gt(Expr::Ident("value".into()), int(i32::MAX as i64)),
                    ),
                    then_body: failed,
                    else_body: None,
                },
                Stmt::Assign {
                    name: "converted".into(),
                    value: Expr::Ident("value".into()),
                },
                Stmt::Return(Some(Expr::Ident("converted".into()))),
            ],
        });
        name
    }

    fn convert_integer(
        &self,
        ty: &str,
        value: &mut Expr,
        scope: &Scope,
        rank: usize,
    ) -> Result<()> {
        if let Some(n) = self.constant(value, scope) {
            self.check_c_value(ty, n)?;
        }
        if rank == 0 {
            *value = Expr::Call(ty.into(), vec![value.clone()]);
            return Ok(());
        }
        let mut distributed = false;
        if let Expr::Call(tag, items) = value {
            if tag == "@array_constructor" {
                for item in items {
                    let rank = self.expression(&mut item.clone(), scope, false)?.rank();
                    self.convert_integer(ty, item, scope, rank)?;
                }
                distributed = true;
            } else if tag == "@output_implied_do" && items.len() >= 5 {
                let mut inner = scope.clone();
                if let Expr::Ident(index) = &items[0] {
                    inner.vars.insert(index.clone(), Ty::Integer(0));
                }
                for item in &mut items[4..] {
                    let rank = self.expression(&mut item.clone(), &inner, false)?.rank();
                    self.convert_integer(ty, item, &inner, rank)?;
                }
                distributed = true;
            }
        }
        *value = Expr::Call(
            if distributed {
                "@enumeration_reference"
            } else {
                "@enum_conversion"
            }
            .into(),
            vec![Expr::Str(ty.into()), int(rank as i64), value.clone()],
        );
        Ok(())
    }
    pub(super) fn is_c_enum(&self, name: &str) -> bool {
        self.types
            .get(name)
            .is_some_and(|tag| tag.starts_with("cenum:"))
    }
    pub(super) fn check_c_value(&self, name: &str, value: i64) -> Result<()> {
        if i32::try_from(value).is_err() {
            bail!("enum `{name}` integer value {value} is outside the supported C int range");
        }
        Ok(())
    }
    /// Only the enumerator itself conveys conformance. A different INTEGER
    /// PARAMETER initialized using it is an ordinary integer constant.
    pub(super) fn member_types(&self, e: &Expr, scope: &Scope) -> HashSet<String> {
        let mut types = HashSet::new();
        visit_expr(&mut e.clone(), &mut |e| {
            if let Expr::Ident(n) = e {
                if let Some(ty) = scope
                    .enum_members
                    .get(n)
                    .or_else(|| self.globals.enum_members.get(n))
                {
                    types.insert(ty.clone());
                }
            }
        });
        types
    }
    pub(super) fn check_value(
        &self,
        target: &Ty,
        value: &mut Expr,
        scope: &Scope,
        prepare: bool,
    ) -> Result<()> {
        let members = self.member_types(value, scope);
        let actual = self.expression(value, scope, prepare)?;
        if let Some(ty) = target.enumeration() {
            if self.is_c_enum(ty) && actual == Ty::Unknown && !self.strict && members.contains(ty) {
                if prepare {
                    *value = Expr::Call(
                        "@enum_conversion".into(),
                        vec![
                            Expr::Str(ty.into()),
                            int(target.rank() as i64),
                            value.clone(),
                        ],
                    );
                }
                return Ok(());
            }
            if self.is_c_enum(ty) && matches!(actual, Ty::Integer(_)) && members.contains(ty) {
                if actual.rank() != 0 && actual.rank() != target.rank() {
                    bail!("enum assignment ranks do not conform");
                }
                if let Some(n) = self.constant(value, scope) {
                    self.check_c_value(ty, n)?;
                }
                if prepare {
                    self.convert_integer(ty, value, scope, actual.rank())?;
                }
                return Ok(());
            }
        }
        self.compatible(target, &actual)
    }
}
