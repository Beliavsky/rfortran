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
            if self.is_c_enum(ty) && matches!(actual, Ty::Integer(_)) && members.contains(ty) {
                if actual.rank() != 0 {
                    bail!("type-conforming integer array expressions for enum assignment are not yet supported");
                }
                if let Some(n) = self.constant(value, scope) {
                    self.check_c_value(ty, n)?;
                }
                if prepare {
                    *value = Expr::Call(ty.into(), vec![value.clone()]);
                }
                return Ok(());
            }
        }
        self.compatible(target, &actual)
    }
}
