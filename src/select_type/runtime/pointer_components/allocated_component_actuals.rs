// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
//! Recognize source scalar components before expanding record storage.
use super::*;
#[derive(Clone)]
struct Member {
    kind: String,
    attrs: Vec<String>,
    rank: usize,
}
pub(super) struct Model {
    types: HashMap<String, HashMap<String, Member>>,
}
impl Model {
    pub(super) fn new(body: &mut [Stmt]) -> Self {
        fn collect(body: &mut [Stmt], types: &mut HashMap<String, HashMap<String, Member>>) {
            for s in body {
                if let Stmt::DerivedType {
                    name, components, ..
                } = s
                {
                    let mut members = HashMap::new();
                    for c in components {
                        if let Stmt::DerivedDecl {
                            type_name,
                            names,
                            attributes,
                            dims,
                            polymorphic: false,
                        } = c
                        {
                            for n in names {
                                members.insert(
                                    n.clone(),
                                    Member {
                                        kind: type_name.clone(),
                                        attrs: attributes.clone(),
                                        rank: dims.len(),
                                    },
                                );
                            }
                        }
                    }
                    types.insert(name.clone(), members);
                } else {
                    for b in children(s) {
                        collect(b, types);
                    }
                }
            }
        }
        let mut types = HashMap::new();
        collect(body, &mut types);
        Self { types }
    }
    fn root(&self, n: &str, declared: &Declarations) -> Option<(String, usize, bool)> {
        let mut fields = n.split('%');
        let root = fields.next()?;
        let ds = declared.get(root)?;
        let mut value = ds.iter().find_map(|s| match s {
            Stmt::DerivedDecl {
                type_name,
                dims,
                attributes,
                ..
            } => Some((
                type_name.clone(),
                dims.len(),
                attributes.iter().any(|a| a == "intent(in)"),
            )),
            _ => None,
        })?;
        value.2 |= ds
            .iter()
            .any(|s| matches!(s,Stmt::DummyIntent{intent,..} if intent=="intent(in)"));
        for field in fields {
            let member = self.types.get(&value.0)?.get(field)?;
            value.0 = member.kind.clone();
            value.1 = member.rank;
        }
        Some(value)
    }
    fn value(&self, e: &Expr, declared: &Declarations) -> Option<(String, usize, bool)> {
        match e {
            Expr::Ident(n) => self.root(n, declared),
            Expr::Call(tag, args) if tag.starts_with("@record_component:") => {
                let mut v = self.value(args.first()?, declared)?;
                for field in tag[18..].split('%') {
                    let m = self.types.get(&v.0)?.get(field)?;
                    v.0 = m.kind.clone();
                    v.1 = m.rank;
                }
                if let Some(Expr::Call(n, sub)) = args.get(1) {
                    if n == "@component_subscripts" {
                        v.1 = v.1.saturating_sub(sub.len());
                    }
                }
                Some(v)
            }
            Expr::Call(n, args) | Expr::Index(n, args) => {
                let mut v = self.root(n, declared)?;
                v.1 = v.1.saturating_sub(args.len());
                Some(v)
            }
            _ => None,
        }
    }
    fn member(&self, e: &Expr, declared: &Declarations) -> Option<(Member, bool)> {
        match e {
            Expr::Ident(n) | Expr::Call(n, _)
                if n.contains('%') && !n.starts_with("@record_component:") =>
            {
                let (owner, field) = n.rsplit_once('%')?;
                let v = self.root(owner, declared)?;
                Some((self.types.get(&v.0)?.get(field)?.clone(), v.2))
            }
            Expr::Call(tag, args) if tag.starts_with("@record_component:") => {
                let v = self.value(args.first()?, declared)?;
                let mut ty = v.0;
                let mut last = None;
                for field in tag[18..].split('%') {
                    let m = self.types.get(&ty)?.get(field)?.clone();
                    ty = m.kind.clone();
                    last = Some(m);
                }
                Some((last?, v.2))
            }
            _ => None,
        }
    }
    pub(super) fn actual(
        &self,
        e: &Expr,
        declared: &Declarations,
    ) -> Result<Option<(Expr, String, bool)>> {
        let Some((m, readonly)) = self.member(e, declared) else {
            return Ok(None);
        };
        if !m.attrs.iter().any(|a| a == "allocatable")
            || !m.attrs.iter().any(|a| a == "@scalar_heap")
        {
            bail!("ALLOCATABLE scalar dummy requires an ALLOCATABLE scalar component actual");
        }
        let descriptor = match e {
            Expr::Ident(n) => id(n),
            Expr::Call(n, a)
                if !n.starts_with("@record_component:")
                    && matches!(a.as_slice(),[Expr::IntLit(i)] if i=="1") =>
            {
                id(n)
            }
            Expr::Call(n, a) if n.starts_with("@record_component:") => {
                Expr::Call(n.clone(), vec![a[0].clone()])
            }
            _ => bail!("ALLOCATABLE scalar dummy requires a whole scalar component"),
        };
        Ok(Some((descriptor, m.kind, readonly)))
    }
    pub(super) fn capture(&self, e: &mut Expr, out: &mut Vec<Stmt>, serial: &mut usize) {
        if let Expr::Call(tag, args) | Expr::Index(tag, args) = e {
            if tag.starts_with("@record_component:") {
                for a in args {
                    self.capture(a, out, serial);
                }
            } else {
                for index in args {
                    if !matches!(index, Expr::IntLit(_)) {
                        *serial += 1;
                        let n = format!("@component_owner_index_{serial}");
                        out.push(Stmt::VarDecl {
                            kind: TypeSpec::Integer(Some(8)),
                            names: vec![n.clone()],
                        });
                        out.push(Stmt::Assign {
                            name: n.clone(),
                            value: index.clone(),
                        });
                        *index = id(&n);
                    }
                }
            }
        }
    }
}
pub(super) fn normalize(program: &mut Program) -> Result<()> {
    let model = Model::new(&mut program.body);
    fn descriptor(e: &mut Expr, declared: &Declarations, model: &Model) {
        if model
            .member(e, declared)
            .is_some_and(|(m, _)| m.attrs.iter().any(|a| a == "@scalar_heap"))
        {
            if let Expr::Call(tag, args) = e {
                if tag.starts_with("@record_component:") {
                    args.truncate(1);
                }
            }
        } else if let Expr::Call(tag, args) = e {
            if tag.starts_with("@keyword:") {
                for a in args {
                    descriptor(a, declared, model);
                }
            }
        }
    }
    fn scope(body: &mut Vec<Stmt>, parent: &Declarations, model: &Model) -> Result<()> {
        let declared = declarations(body, parent);
        for s in body {
            for e in expressions(s) {
                visit_expr(e, &mut |e| {
                    if let Some((m, _)) = model.member(e, &declared) {
                        if m.attrs.iter().any(|a| a == "@scalar_heap") {
                            if let Expr::Call(tag, args) = e {
                                if tag.starts_with("@record_component:") && args.len() == 1 {
                                    args.push(Expr::Call(
                                        "@component_subscripts".into(),
                                        vec![int(1)],
                                    ));
                                }
                            }
                        }
                    }
                    if let Expr::Call(tag, args) = e {
                        if matches!(tag.as_str(), "allocated" | "present" | "move_alloc") {
                            for a in args {
                                descriptor(a, &declared, model);
                            }
                        }
                    }
                });
            }
            if let Stmt::CallSub { name, args } = s {
                if matches!(
                    name.as_str(),
                    "@deallocate" | "move_alloc" | "@move_allocation_storage"
                ) {
                    for a in args {
                        descriptor(a, &declared, model);
                    }
                }
            }
            for b in children(s) {
                scope(b, &declared, model)?;
            }
        }
        Ok(())
    }
    let mut globals = Declarations::new();
    for s in &program.body {
        if let Stmt::Module { body, .. } = s {
            globals.extend(declarations(body, &Declarations::new()));
        }
    }
    scope(&mut program.body, &globals, &model)
}
