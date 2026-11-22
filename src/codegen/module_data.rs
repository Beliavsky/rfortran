// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! Declare shared module data before generating any function that binds it.
use super::*;

fn bytes(initial: &crate::modules::Storage, little: bool) -> Result<Vec<u8>> {
    // A deferred-length CHARACTER scalar: the pointer and length of its buffer.
    if crate::substrings::deferred(&initial.kind) {
        return Ok(vec![0; 16]);
    }
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
/// Storage that every object using it defines (COMMON blocks, host storage),
/// which the linker must merge. On COFF, where weak definitions do not merge,
/// `finish` makes a copy that is zero or has no initial values a common
/// symbol of `size` bytes, which the linker zero-fills and merges with the
/// other copies, and a copy with values, as BLOCK DATA gives, an ordinary
/// definition, which takes the place of the common symbols. (A weak COFF
/// definition would be only a default, which a common symbol overrides.)
pub(super) struct Shared {
    id: cranelift_module::DataId,
    /// The size of a common symbol; None for a definition.
    common: Option<u64>,
}

fn walk(
    obj: &mut ObjectModule,
    body: &[IStmt],
    owners: &HashSet<String>,
    types: &mut HashMap<String, TypeSpec>,
    shared: &mut Vec<Shared>,
) -> Result<()> {
    // Child scopes inherit types, but must not change the parent's bindings.
    // Save only declarations introduced here instead of cloning every visible
    // type at every branch/loop of a heavily specialized main program.
    let mut previous = Vec::new();
    for statement in body {
        if let IStmt::DeclVar(kind, name, _) = statement {
            previous.push((name, types.insert(name.clone(), kind.clone())));
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
                // So does the storage of saved ALLOCATABLE and POINTER locals of
                // such procedures (see crate::saved_storage), and of the host
                // variables of their internal procedures (see
                // crate::host_storage); that of module procedures belongs to
                // their module, whose object defines it.
                let saved_owner = crate::saved_storage::owner(module);
                let private_saved = module == "@pointer_saved" || saved_owner == Some("");
                // COMMON block storage (see crate::common) is defined by every
                // object that uses the block; weak definitions let objects
                // compiled separately share one copy (on COFF, common symbols:
                // see Shared).
                // So is the storage of the pools of recursive types (see
                // crate::recursive_types) and of the strings passed to
                // recursive procedures (see crate::recursive_characters),
                // which no module file describes.
                let common = crate::common::is_block_module(module) || module == crate::recursive_types::MODULE
                    || module == crate::recursive_characters::MODULE;
                let owned = private_saved || common || owners.contains(saved_owner.unwrap_or(module));
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
                    let contents = bytes(&initial, obj.isa().endianness() == ir::Endianness::Little)?;
                    let coff = obj.isa().triple().binary_format == target_lexicon::BinaryFormat::Coff;
                    if common && coff {
                        let zero = !initial.initialized || contents.iter().all(|b| *b == 0);
                        shared.push(Shared { id, common: zero.then_some(contents.len() as u64) });
                        if zero {
                            continue;
                        }
                    }
                    let mut data = DataDescription::new();
                    data.define(contents.into_boxed_slice());
                    data.set_align(8);
                    obj.define_data(id, &data)?;
                }
            }
            IStmt::Block { body } | IStmt::Do { body, .. } | IStmt::DoWhile { body, .. } => {
                walk(obj, body, owners, types, shared)?
            }
            IStmt::If {
                then_body,
                else_body,
                ..
            } => {
                walk(obj, then_body, owners, types, shared)?;
                if let Some(body) = else_body {
                    walk(obj, body, owners, types, shared)?;
                }
            }
            IStmt::SelectCase { cases, default, .. } => {
                for case in cases {
                    walk(obj, &case.body, owners, types, shared)?;
                }
                if let Some(body) = default {
                    walk(obj, body, owners, types, shared)?;
                }
            }
            _ => {}
        }
    }
    for (name, old) in previous.into_iter().rev() {
        if let Some(kind) = old {
            types.insert(name.clone(), kind);
        } else {
            types.remove(name);
        }
    }
    Ok(())
}
pub(super) fn declare(
    obj: &mut ObjectModule,
    module: &Module,
    owners: &HashSet<String>,
) -> Result<Vec<Shared>> {
    let mut shared = vec![];
    for function in &module.funcs {
        walk(obj, &function.body, owners, &mut HashMap::new(), &mut shared)?;
    }
    Ok(shared)
}

/// Makes the shared storage of a COFF object common symbols and ordinary
/// definitions.
pub(super) fn finish(product: &mut cranelift_object::ObjectProduct, shared: &[Shared]) {
    for s in shared {
        let Some((symbol, _)) = product.data_objects[s.id] else { continue };
        let entry = product.object.symbol_mut(symbol);
        entry.weak = false;
        if let Some(size) = s.common {
            entry.section = object::write::SymbolSection::Common;
            entry.size = size;
        }
    }
}
