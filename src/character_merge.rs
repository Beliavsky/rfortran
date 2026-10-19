// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
//! Materialize CHARACTER MERGE before CHARACTER arrays become scalar slots.
use super::*;

impl Lower {
    pub(super) fn merge_character(
        &mut self,
        e: &mut Expr,
        symbols: &mut Symbols,
        prefix: &mut Vec<Stmt>,
    ) -> Result<bool> {
        let Expr::Call(name, args) = e else {
            return Ok(false);
        };
        if name != "merge" || self.returns.contains_key(name) || symbols.contains_key(name) {
            return Ok(false);
        }
        if args.len() != 3 {
            bail!("MERGE expects TSOURCE, FSOURCE and MASK");
        }
        let first = self.view(&args[0], symbols);
        let second = self.view(&args[1], symbols);
        if !first
            .as_ref()
            .is_ok_and(|v| matches!(v.kind, TypeSpec::Character(_)))
            && !second
                .as_ref()
                .is_ok_and(|v| matches!(v.kind, TypeSpec::Character(_)))
        {
            return Ok(false);
        }
        let views = (|| -> Result<Vec<Value>> {
            Ok(vec![first?, second?, self.view(&args[2], symbols)?])
        })();
        let views = match views {
            Ok(v) => v,
            Err(_) if self.defer_calls => return Ok(false),
            Err(e) => return Err(e),
        };
        if !matches!(views[0].kind, TypeSpec::Character(_))
            || !matches!(views[1].kind, TypeSpec::Character(_))
            || length(&views[0].kind)? != length(&views[1].kind)?
        {
            bail!("MERGE sources must have the same type, kind and CHARACTER length");
        }
        if views[2].kind != TypeSpec::Logical {
            bail!("MERGE MASK must be LOGICAL");
        }
        if views.iter().any(|v| v.active.is_some()) {
            if self.defer_calls {
                return Ok(false);
            }
            bail!("CHARACTER MERGE currently requires fixed array extents");
        }
        let mut count = None;
        let mut shape = None;
        for (arg, view) in args.iter().zip(&views) {
            if let Some(items) = &view.items {
                if count.is_some_and(|n| n != items.len()) {
                    bail!("MERGE argument shapes do not conform");
                }
                count = Some(items.len());
                let dims = crate::multidim::fixed_actual_shape(arg, symbols)?
                    .unwrap_or_else(|| vec![items.len() as i64]);
                if shape.as_ref().is_some_and(|previous| previous != &dims) {
                    bail!("MERGE argument shapes do not conform");
                }
                shape = Some(dims);
            }
        }
        self.serial += 1;
        let result = format!("@character_merge_{}", self.serial);
        let kind = views[0].kind.clone();
        let dims = shape.unwrap_or_default();
        prefix.push(if count.is_some() {
            Stmt::ArrayDecl {
                kind: kind.clone(),
                name: result.clone(),
                dims: dims.iter().copied().map(int).collect(),
            }
        } else {
            Stmt::VarDecl {
                kind: kind.clone(),
                names: vec![result.clone()],
            }
        });
        // A scalar mask expression is a single argument, even when broadcast.
        let scalar_mask = if views[2].items.is_none() {
            let mask = format!("@character_merge_mask_{}", self.serial);
            prefix.push(Stmt::VarDecl {
                kind: TypeSpec::Logical,
                names: vec![mask.clone()],
            });
            prefix.push(Stmt::Assign {
                name: mask.clone(),
                value: args[2].clone(),
            });
            Some(Expr::Ident(mask))
        } else {
            None
        };
        for i in 0..count.unwrap_or(1) {
            let item = |which: usize| {
                views[which]
                    .items
                    .as_ref()
                    .map_or_else(|| args[which].clone(), |v| v[i].clone())
            };
            let assign = |value| {
                if count.is_some() {
                    Stmt::AssignIndex {
                        name: result.clone(),
                        indices: array_indices(i, &dims),
                        value,
                    }
                } else {
                    Stmt::Assign {
                        name: result.clone(),
                        value,
                    }
                }
            };
            prefix.push(Stmt::If {
                cond: scalar_mask.clone().unwrap_or_else(|| item(2)),
                then_body: vec![assign(item(0))],
                else_body: Some(vec![assign(item(1))]),
            });
        }
        symbols.insert(result.clone(), (kind, dims.into_iter().map(int).collect()));
        *e = Expr::Ident(result);
        Ok(true)
    }
}
