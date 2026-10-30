// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! Declare shared module data before generating any function that binds it.
use super::*;

fn bytes(initial: &crate::modules::Storage, little: bool) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    let values = if initial.deferred {
        vec![crate::ast::Expr::IntLit("0".into())]
    } else {
        initial.values.clone()
    };
    for value in values {
        if let TypeSpec::Character(length) = initial.kind {
            let mut text = crate::parameters::character_constant(&value)?.into_bytes();
            let length = length.unwrap_or(1);
            text.resize(length, b' ');
            text.truncate(length);
            text.push(0);
            bytes.extend(text);
        } else {
            let bits = if initial.deferred {
                0
            } else {
                match initial.kind {
                    TypeSpec::Real | TypeSpec::DoublePrecision => {
                        crate::parameters::real_constant(&value)?.to_bits()
                    }
                    TypeSpec::Logical => {
                        if let crate::ast::Expr::Logical(v) = value {
                            u64::from(v)
                        } else {
                            bail!("invalid LOGICAL module initializer");
                        }
                    }
                    _ => crate::parameters::integer_constant(&value)? as u64,
                }
            };
            if initial.kind == TypeSpec::Logical && initial.rank == 0 {
                bytes.push(bits as u8);
            } else if little {
                bytes.extend(bits.to_le_bytes());
            } else {
                bytes.extend(bits.to_be_bytes());
            }
        }
    }
    if bytes.is_empty() {
        bytes.resize(8, 0);
    }
    Ok(bytes)
}
fn constant(value: &IExpr) -> Result<crate::ast::Expr> {
    use crate::ast::Expr;
    Ok(match value {
        IExpr::IntLit(v) => Expr::IntLit(v.clone()),
        IExpr::RealLit(v) => Expr::RealLit(v.clone()),
        IExpr::Logical(v) => Expr::Logical(*v),
        IExpr::Str(v) => Expr::Str(v.clone()),
        IExpr::Call(tag, args) if matches!(tag.as_str(), "dble" | "@integer_literal") => {
            constant(&args[0])?
        }
        _ => bail!("saved initializer must be a resolved scalar constant"),
    })
}
fn walk(
    obj: &mut ObjectModule,
    body: &[IStmt],
    owners: &HashSet<String>,
    types: &mut HashMap<String, TypeSpec>,
) -> Result<()> {
    for statement in body {
        if let IStmt::DeclVar(kind, name, _) = statement {
            types.insert(name.clone(), kind.clone());
        }
    }
    for statement in body {
        match statement {
            IStmt::Call(name, args) if name == "@module_bind" || name == "@saved_bind" => {
                let [IExpr::Ident(local), IExpr::Str(key), value] = args.as_slice() else {
                    bail!("invalid module storage binding");
                };
                let Some(module) = key
                    .strip_prefix("@module$")
                    .and_then(|s| s.split('$').next())
                else {
                    continue;
                };
                let symbol = format!(
                    ".rfort.{}.{key}",
                    if name == "@module_bind" {
                        "module"
                    } else {
                        "saved"
                    }
                );
                if obj.get_name(&symbol).is_some() {
                    continue;
                }
                // Compiler-promoted SAVE targets in main/internal/external
                // procedures have translation-unit storage. Module procedures
                // instead retain their real module owner and exported symbol.
                let private_saved = module == "@pointer_saved";
                // COMMON block storage (see crate::common) is defined by every
                // object that uses the block; weak definitions let objects
                // compiled separately share one copy.
                let common = module.starts_with("rfort_common_") || module == "rfort_blank_common";
                let owned = private_saved || common || owners.contains(module);
                let id = obj.declare_data(
                    &symbol,
                    if private_saved {
                        Linkage::Local
                    } else if common {
                        Linkage::Preemptible
                    } else if owned {
                        Linkage::Export
                    } else {
                        Linkage::Import
                    },
                    true,
                    false,
                )?;
                if owned {
                    let initial = if name == "@module_bind" {
                        let IExpr::Str(encoded) = value else {
                            bail!("invalid module initializer");
                        };
                        serde_json::from_str(encoded)?
                    } else {
                        crate::modules::Storage {
                            kind: types
                                .get(local)
                                .context("missing saved variable type")?
                                .clone(),
                            values: vec![constant(value)?],
                            initialized: true,
                            rank: 0,
                            deferred: false,
                        }
                    };
                    let mut data = DataDescription::new();
                    data.define(
                        bytes(&initial, obj.isa().endianness() == ir::Endianness::Little)?
                            .into_boxed_slice(),
                    );
                    data.set_align(8);
                    obj.define_data(id, &data)?;
                }
            }
            IStmt::Block { body } | IStmt::Do { body, .. } | IStmt::DoWhile { body, .. } => {
                walk(obj, body, owners, &mut types.clone())?
            }
            IStmt::If {
                then_body,
                else_body,
                ..
            } => {
                walk(obj, then_body, owners, &mut types.clone())?;
                if let Some(body) = else_body {
                    walk(obj, body, owners, &mut types.clone())?;
                }
            }
            IStmt::SelectCase { cases, default, .. } => {
                for case in cases {
                    walk(obj, &case.body, owners, &mut types.clone())?;
                }
                if let Some(body) = default {
                    walk(obj, body, owners, &mut types.clone())?;
                }
            }
            _ => {}
        }
    }
    Ok(())
}
pub(super) fn declare(
    obj: &mut ObjectModule,
    module: &Module,
    owners: &HashSet<String>,
) -> Result<()> {
    for function in &module.funcs {
        walk(obj, &function.body, owners, &mut HashMap::new())?;
    }
    Ok(())
}
