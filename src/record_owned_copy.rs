// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
//! Deep-copy array components through their record type, never raw owned handles.
use super::*;
impl Lower {
    pub(super) fn needs_owned_copy(&self, ty: &str) -> bool {
        self.component_definitions
            .get(ty)
            .is_some_and(|components| {
                components.iter().any(|s| match s {
                    Stmt::DerivedDecl {
                        type_name, dims, attributes, ..
                    } => {
                        if attributes.iter().any(|a|a=="pointer" || a=="allocatable" && dims.is_empty()) {return false;}
                        if attributes.iter().any(|a|a=="allocatable") && attributes.iter().any(|a|a=="@scalar_heap") {return true;}
                        if dims.is_empty() {
                            self.needs_owned_copy(type_name)
                        } else {
                            self.types
                                .get(type_name)
                                .is_some_and(|fields| fields.iter().any(|f| f.heap.is_some()))
                        }
                    }
                    _ => false,
                })
            })
    }
    pub(super) fn copy_owned_container(
        &mut self,
        to: &str,
        from: &str,
        ty: &str,
        objects: &mut Objects,
        out: &mut Vec<Stmt>,
    ) -> Result<()> {
        if to == from {
            return Ok(());
        }
        fn copy(lower: &Lower, to: &str, from: &str, ty: &str, out: &mut Vec<Stmt>) -> Result<()> {
            let definitions = lower
                .component_definitions
                .get(ty)
                .context("missing owned component layout")?;
            let mut members = vec![];
            for s in definitions {
                match s {
                    Stmt::VarDecl { names, .. } => {
                        for n in names {
                            members.push((n.clone(), vec![], None, vec![]));
                        }
                    }
                    Stmt::InitializedDecl { entities, .. } => {
                        for (n, _) in entities {
                            members.push((n.clone(), vec![], None, vec![]));
                        }
                    }
                    Stmt::ArrayDecl { name, dims, .. } => {
                        members.push((name.clone(), dims.clone(), None, vec![]))
                    }
                    Stmt::DerivedDecl {
                        names,
                        dims,
                        type_name,
                        attributes,
                        ..
                    } => {
                        for n in names {
                            members.push((
                                n.clone(),
                                dims.clone(),
                                Some(type_name.clone()),
                                attributes.clone(),
                            ));
                        }
                    }
                    _ => {}
                }
            }
            for (name, dims, child, mut attrs) in members {
                attrs.extend(definitions.iter().filter_map(|s| match s {
                    Stmt::DummyIntent { name: n, intent } if n == &name => Some(intent.clone()),
                    _ => None,
                }));
                let target = format!("{to}%{name}");
                let source = format!("{from}%{name}");
                let id = |n: &str| Expr::Ident(n.into());
                let test = |function: &str, n: &str| Expr::Call(function.into(), vec![id(n)]);
                let call = |name: &str, args| Stmt::CallSub {
                    name: name.into(),
                    args,
                };
                let guarded = |cond, body| Stmt::If {
                    cond,
                    then_body: body,
                    else_body: None,
                };
                if attrs.iter().any(|a| a == "pointer") {
                    out.push(Stmt::If {
                        cond: test("associated", &source),
                        then_body: vec![call("@pointer_associate", vec![id(&target), id(&source)])],
                        else_body: Some(vec![call("@nullify", vec![id(&target)])]),
                    });
                } else if attrs.iter().any(|a| a == "allocatable") {
                    out.push(guarded(
                        test("allocated", &target),
                        vec![call("@deallocate", vec![id(&target)])],
                    ));
                    let bounds = (1..=dims.len())
                        .map(|axis| {
                            Expr::Call(
                                "@section_subscript".into(),
                                vec![Expr::Section {
                                    name: target.clone(),
                                    lower: Some(Box::new(Expr::Call(
                                        "lbound".into(),
                                        vec![id(&source), Expr::IntLit(axis.to_string())],
                                    ))),
                                    upper: Some(Box::new(Expr::Call(
                                        "ubound".into(),
                                        vec![id(&source), Expr::IntLit(axis.to_string())],
                                    ))),
                                    stride: None,
                                }],
                            )
                        })
                        .collect();
                    out.push(guarded(
                        test("allocated", &source),
                        vec![
                            call("@allocate", vec![Expr::Call(target.clone(), bounds)]),
                            Stmt::Assign {
                                name: target,
                                value: id(&source),
                            },
                        ],
                    ));
                } else if let Some(field)=name.strip_prefix("@allocated_component_epoch_") {
                    // This is an intrinsic deep copy, not descriptor transport
                    // for a borrowed actual. Each copied allocation gets its
                    // own identity, including section and vector destinations.
                    out.push(Stmt::Assign{name:target.clone(),value:Expr::IntLit("0".into())});
                    out.push(guarded(test("allocated",&format!("{to}%{field}")),vec![call("@heap_identity",vec![id(&target)])]));
                } else if dims.is_empty() && child.is_some() {
                    copy(lower, &target, &source, child.as_ref().unwrap(), out)?;
                } else {
                    out.push(Stmt::Assign {
                        name: target,
                        value: id(&source),
                    });
                }
            }
            Ok(())
        }
        let mut generated = vec![];
        copy(self, to, from, ty, &mut generated)?;
        self.body(&mut generated, objects)?;
        out.extend(generated);
        Ok(())
    }
}
