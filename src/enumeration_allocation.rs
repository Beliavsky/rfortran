// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
//! Check allocation while nominal types and declaration attributes still exist.
use super::*;

fn raw(mut e: &Expr) -> &Expr {
    loop {
        if let Some((_, value)) = crate::named_arguments::keyword(e) {
            e = value;
            continue;
        }
        if let Expr::Call(n, a) = e {
            if n == "@enumeration_reference" {
                e = &a[2];
                continue;
            }
        }
        return e;
    }
}
impl Context {
    fn is_allocatable(&self, name: &str, scope: &Scope) -> bool {
        if scope.allocatables.contains(name) || self.globals.allocatables.contains(name) {
            return true;
        }
        if let Some((object, field)) = name.rsplit_once('%') {
            if let Ty::Derived(ty, _) = self.name(object, scope) {
                return self
                    .allocatable_fields
                    .get(&ty)
                    .is_some_and(|fields| fields.contains(field));
            }
        }
        false
    }
    pub(super) fn require_allocatable(&self, e: &Expr, scope: &Scope) -> Result<()> {
        if let Expr::Ident(name) = raw(e) {
            if self.is_allocatable(name, scope) {
                return Ok(());
            }
        }
        bail!("argument must be a whole ALLOCATABLE variable, not a section or expression")
    }
    fn allocation_object(&self, e: &Expr, scope: &Scope, allocate: bool) -> Result<Ty> {
        let name = match raw(e) {
            Expr::Ident(n) => n,
            Expr::Call(n, _) | Expr::Index(n, _) | Expr::Section { name: n, .. } if allocate => n,
            _ => bail!("allocation object must designate a whole ALLOCATABLE variable"),
        };
        let ty = self.name(name, scope);
        if ty.enumeration().is_some() {
            if !self.is_allocatable(name, scope) {
                bail!("enumeration allocation object `{name}` must have the ALLOCATABLE attribute");
            }
            if scope
                .readonly
                .contains(name.split('%').next().unwrap_or(name))
            {
                bail!("cannot change allocation of INTENT(IN) variable `{name}`");
            }
        }
        Ok(ty)
    }
    fn allocation_bounds(&self, e: &Expr, scope: &Scope) -> Result<()> {
        match raw(e) {
            Expr::Section {
                lower,
                upper,
                stride,
                ..
            } => {
                for bound in [lower, upper, stride].into_iter().flatten() {
                    self.allocation_bounds(bound, scope)?;
                }
            }
            Expr::Call(n, a) if n == "@section_subscript" || n == "@declared_bounds" => {
                for bound in a {
                    self.allocation_bounds(bound, scope)?;
                }
            }
            bound => {
                if self
                    .expression(&mut bound.clone(), scope, false)?
                    .enumeration()
                    .is_some()
                {
                    bail!("allocation bounds must be INTEGER, not enumeration values");
                }
            }
        }
        Ok(())
    }
    pub(super) fn check_allocation(
        &self,
        name: &str,
        args: &[Expr],
        scope: &Scope,
    ) -> Result<bool> {
        if name == "move_alloc"
            && !self.procedures.contains_key(name)
            && !self.generics.contains(name)
        {
            let mut actuals = [None, None];
            for (i, arg) in args.iter().enumerate() {
                let (position, value) =
                    if let Some((key, value)) = crate::named_arguments::keyword(arg) {
                        (
                            match key {
                                "from" => 0,
                                "to" => 1,
                                _ => bail!("unsupported MOVE_ALLOC keyword `{key}`"),
                            },
                            value,
                        )
                    } else {
                        (i, arg)
                    };
                if position >= 2 || actuals[position].replace(value).is_some() {
                    bail!("MOVE_ALLOC requires FROM and TO exactly once");
                }
            }
            let [Some(from), Some(to)] = actuals else {
                bail!("MOVE_ALLOC requires FROM and TO")
            };
            let a = self.allocation_object(from, scope, false)?;
            let b = self.allocation_object(to, scope, false)?;
            self.compatible(&a, &b)?;
            if a.enumeration().is_some() || b.enumeration().is_some() {
                self.require_allocatable(from, scope)?;
                self.require_allocatable(to, scope)?;
                if a.rank() != b.rank() {
                    bail!("MOVE_ALLOC arguments must have the same rank");
                }
            }
            return Ok(true);
        }
        if !matches!(name, "@allocate" | "@deallocate") {
            return Ok(false);
        }
        let mut objects = vec![];
        let mut shape = None;
        for arg in args {
            if let Some((key, value)) = crate::named_arguments::keyword(arg) {
                let ty = self.expression(&mut value.clone(), scope, false)?;
                if matches!(key, "source" | "mold") {
                    if shape.replace(ty).is_some() {
                        bail!("ALLOCATE permits only one of SOURCE= and MOLD=");
                    }
                } else if ty.enumeration().is_some() {
                    bail!("allocation control `{key}` cannot have enumeration type");
                }
            } else {
                objects.push(self.allocation_object(arg, scope, name == "@allocate")?);
                if name == "@allocate" {
                    match raw(arg) {
                        Expr::Call(_, dims) | Expr::Index(_, dims) => {
                            for bound in dims {
                                self.allocation_bounds(bound, scope)?;
                            }
                        }
                        Expr::Section { .. } => self.allocation_bounds(arg, scope)?,
                        _ => {}
                    }
                }
            }
        }
        if let Some(source) = shape {
            for target in objects {
                self.compatible(&target, &source)?;
                if (target.enumeration().is_some() || source.enumeration().is_some())
                    && source.rank() != 0
                    && target.rank() != source.rank()
                {
                    bail!("ALLOCATE SOURCE/MOLD rank must match the allocation object");
                }
            }
        }
        Ok(true)
    }
}
