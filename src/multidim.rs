//! Lower statically known multidimensional arrays to column-major vector storage.
use crate::ast::{BinOp, CaseItem, Expr, Program, Stmt, TypeSpec};
use anyhow::{bail, Context, Result};
use std::collections::HashMap;

type Shapes = HashMap<String, Vec<i64>>;
fn int(n: i64) -> Expr {
    Expr::IntLit(n.to_string())
}
fn bin(op: BinOp, a: Expr, b: Expr) -> Expr {
    Expr::Bin(op, Box::new(a), Box::new(b))
}
fn total(dims: &[i64]) -> Result<i64> {
    dims.iter().try_fold(1i64, |n, d| {
        n.checked_mul(*d)
            .context("array element count overflows INTEGER(8)")
    })
}

#[derive(Default)]
struct Lowerer {
    guards: Vec<Stmt>,
    types: HashMap<String, TypeSpec>,
    functions: HashMap<String, TypeSpec>,
    module_arrays: std::collections::HashSet<String>,
}
impl Lowerer {
    fn check_association(&self, name: &str) -> Result<()> {
        if self.module_arrays.contains(name)
            && !self.types.contains_key(name)
            && !self.functions.contains_key(name)
        {
            bail!("USE-associated multidimensional array `{name}` is not yet supported");
        }
        Ok(())
    }
    fn integer_expr(&self, e: &Expr, shapes: &Shapes) -> bool {
        match e {
            Expr::IntLit(n) => n.parse::<i64>().is_ok(),
            Expr::Ident(n) => {
                matches!(
                    self.types.get(n),
                    Some(TypeSpec::Integer(None | Some(1 | 2 | 4 | 8)))
                ) && !shapes.contains_key(n)
            }
            Expr::Bin(op, a, b) => {
                matches!(
                    op,
                    BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Pow
                ) && self.integer_expr(a, shapes)
                    && self.integer_expr(b, shapes)
            }
            Expr::Un(crate::ast::UnOp::Neg, a) => self.integer_expr(a, shapes),
            Expr::Call(n, args) | Expr::Index(n, args) => {
                if let Some(dims) = shapes.get(n) {
                    return matches!(
                        self.types.get(n),
                        Some(TypeSpec::Integer(None | Some(1 | 2 | 4 | 8)))
                    ) && args.len() == dims.len()
                        && args.iter().all(|a| self.integer_expr(a, shapes));
                }
                if let Some(kind) = self.functions.get(n) {
                    return matches!(kind, TypeSpec::Integer(_));
                }
                match n.as_str() {
                    "size" | "len" | "count" => true,
                    "int" | "nint" | "floor" | "ceiling" => args
                        .first()
                        .is_some_and(|e| Self::shape(e, shapes).is_ok_and(|s| s.is_none())),
                    "abs" | "mod" | "modulo" | "min" | "max" => {
                        args.iter().all(|a| self.integer_expr(a, shapes))
                    }
                    _ => false,
                }
            }
            _ => false,
        }
    }
    fn check_index_types(&self, name: &str, indices: &[Expr], shapes: &Shapes) -> Result<()> {
        fn has_procedure(e: &Expr, functions: &HashMap<String, TypeSpec>, shapes: &Shapes) -> bool {
            match e {
                Expr::Call(n, args) | Expr::Index(n, args) => {
                    (!shapes.contains_key(n) && functions.contains_key(n))
                        || args.iter().any(|e| has_procedure(e, functions, shapes))
                }
                Expr::Bin(_, a, b) => {
                    has_procedure(a, functions, shapes) || has_procedure(b, functions, shapes)
                }
                Expr::Un(_, a) => has_procedure(a, functions, shapes),
                _ => false,
            }
        }
        if indices
            .iter()
            .any(|e| has_procedure(e, &self.functions, shapes))
        {
            bail!("procedure calls in multidimensional subscripts are not yet supported");
        }
        if !indices.iter().all(|e| self.integer_expr(e, shapes)) {
            bail!("subscripts of multidimensional array `{name}` must be explicitly typed scalar INTEGER expressions");
        }
        Ok(())
    }
    fn indices(&mut self, name: &str, indices: &mut Vec<Expr>, dims: &[i64]) -> Result<()> {
        if indices.len() != dims.len() {
            bail!(
                "array `{name}` requires {} subscripts, got {}",
                dims.len(),
                indices.len()
            );
        }
        let mut offset = int(1);
        let mut stride = 1i64;
        for (axis, (e, extent)) in indices.iter().zip(dims).enumerate() {
            let checked = if let Ok(index) = crate::parameters::integer_constant(e) {
                if index < 1 || index > *extent {
                    bail!(
                        "array `{name}` dimension {}: subscript {index} outside 1:{extent}",
                        axis + 1
                    );
                }
                int(index)
            } else {
                // A function call checks each dynamic subscript exactly once, including
                // in loop conditions and nested expressions. Checking only the flattened
                // offset would miss invalid subscripts that alias another column.
                let function = format!("@matrix_bound_{}", self.guards.len());
                let parameter = format!("@matrix_index_{}", self.guards.len());
                let value = Expr::Ident(parameter.clone());
                self.guards.push(Stmt::Function {
                    name: function.clone(),
                    params: vec![parameter.clone()],
                    pure: false,
                    return_type: Some(TypeSpec::Integer(Some(8))),
                    body: vec![
                        Stmt::VarDecl {
                            kind: TypeSpec::Integer(Some(8)),
                            names: vec![parameter],
                        },
                        Stmt::If {
                            cond: bin(
                                BinOp::Or,
                                bin(BinOp::Lt, value.clone(), int(1)),
                                bin(BinOp::Gt, value.clone(), int(*extent)),
                            ),
                            then_body: vec![Stmt::CallSub {
                                name: "@error_stop".into(),
                                args: vec![Expr::Str(format!(
                                    "array `{name}` dimension {}: subscript outside 1:{extent}",
                                    axis + 1
                                ))],
                            }],
                            else_body: None,
                        },
                        Stmt::Return(Some(value)),
                    ],
                });
                Expr::Call(function, vec![e.clone()])
            };
            offset = bin(
                BinOp::Add,
                offset,
                bin(BinOp::Mul, bin(BinOp::Sub, checked, int(1)), int(stride)),
            );
            stride = stride
                .checked_mul(*extent)
                .context("array stride overflow")?;
        }
        if let Ok(n) = crate::parameters::integer_constant(&offset) {
            offset = int(n);
        }
        *indices = vec![offset];
        Ok(())
    }

    fn expression(&mut self, e: &mut Expr, shapes: &Shapes) -> Result<()> {
        match e {
            Expr::Ident(n) => self.check_association(n)?,
            Expr::Call(n, args) if n == "size" => {
                let Some(Expr::Ident(array)) = args.first() else {
                    bail!("SIZE currently requires a whole array");
                };
                self.check_association(array)?;
                let Some(dims) = shapes.get(array) else {
                    return Ok(());
                };
                let value = match args.as_slice() {
                    [_] => total(dims)?,
                    [_, dim] => {
                        let axis = crate::parameters::integer_constant(dim)
                            .context("SIZE dimension must currently be constant")?;
                        if axis < 1 || axis as usize > dims.len() {
                            bail!("SIZE dimension must be between 1 and {}", dims.len());
                        }
                        dims[axis as usize - 1]
                    }
                    _ => bail!("SIZE expects an array and optional dimension"),
                };
                *e = int(value);
            }
            Expr::Call(n, args) | Expr::Index(n, args) => {
                self.check_association(n)?;
                if shapes.get(n).is_some_and(|d| d.len() > 1) {
                    self.check_index_types(n, args, shapes)?;
                }
                for a in args.iter_mut() {
                    self.expression(a, shapes)?;
                }
                if let Some(dims) = shapes.get(n).filter(|d| d.len() > 1) {
                    self.indices(n, args, dims)?;
                }
            }
            Expr::Bin(_, a, b) => {
                self.expression(a, shapes)?;
                self.expression(b, shapes)?;
            }
            Expr::Un(_, a) => self.expression(a, shapes)?,
            Expr::Section {
                name,
                lower,
                upper,
                stride,
            } => {
                if shapes.get(name).is_some_and(|d| d.len() > 1) {
                    bail!("multidimensional sections of `{name}` are not yet supported");
                }
                for e in [lower, upper, stride].into_iter().flatten() {
                    self.expression(e, shapes)?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn shape(e: &Expr, shapes: &Shapes) -> Result<Option<Vec<i64>>> {
        match e {
            Expr::Ident(n) => Ok(shapes.get(n).cloned()),
            Expr::Bin(_, a, b) => {
                let a = Self::shape(a, shapes)?;
                let b = Self::shape(b, shapes)?;
                if let (Some(a), Some(b)) = (&a, &b) {
                    if a != b {
                        bail!("nonconforming array shapes {a:?} and {b:?}");
                    }
                }
                Ok(a.or(b))
            }
            Expr::Un(_, a) => Self::shape(a, shapes),
            Expr::Call(n, args) if crate::intrinsics::math_arity(n).is_some() || n == "abs" => args
                .first()
                .map(|e| Self::shape(e, shapes))
                .unwrap_or(Ok(None)),
            _ => Ok(None),
        }
    }

    fn body(&mut self, body: &mut [Stmt], inherited: &Shapes) -> Result<()> {
        let mut shapes = inherited.clone();
        let inherited_types = self.types.clone();
        for s in body.iter() {
            match s {
                Stmt::VarDecl { names, kind } => {
                    for n in names {
                        shapes.remove(n);
                        self.types.insert(n.clone(), kind.clone());
                    }
                }
                Stmt::ArrayDecl { name, kind, dims } => {
                    self.types.insert(name.clone(), kind.clone());
                    shapes.remove(name);
                    let extents = dims
                        .iter()
                        .map(crate::parameters::integer_constant)
                        .collect::<Result<Vec<_>>>();
                    if dims.len() > 1 {
                        if !matches!(
                            kind,
                            TypeSpec::Integer(None | Some(1 | 2 | 4 | 8))
                                | TypeSpec::Real
                                | TypeSpec::DoublePrecision
                        ) {
                            bail!("multidimensional array `{name}` requires a supported numeric element type");
                        }
                        let extents = extents.with_context(|| {
                            format!(
                                "multidimensional array `{name}` requires a statically known shape"
                            )
                        })?;
                        if extents.iter().any(|d| *d < 1) {
                            bail!("multidimensional array `{name}` currently requires positive extents");
                        }
                        total(&extents)?;
                        shapes.insert(name.clone(), extents);
                    } else if let Ok(extents) = extents {
                        shapes.insert(name.clone(), extents);
                    }
                }
                _ => {}
            }
        }
        for s in body {
            match s {
                Stmt::ArrayDecl { name, dims, .. } => {
                    if let Some(shape) = shapes.get(name).filter(|d| d.len() > 1) {
                        *dims = vec![int(total(shape)?)];
                    }
                }
                Stmt::Assign { name, value } => {
                    self.check_association(name)?;
                    if let (Some(a), Some(b)) = (shapes.get(name), Self::shape(value, &shapes)?) {
                        if *a != b {
                            bail!("nonconforming array assignment to `{name}`: {a:?} and {b:?}");
                        }
                    }
                    self.expression(value, &shapes)?;
                }
                Stmt::AssignIndex {
                    name,
                    indices,
                    value,
                } => {
                    self.check_association(name)?;
                    if shapes.get(name).is_some_and(|d| d.len() > 1) {
                        self.check_index_types(name, indices, &shapes)?;
                    }
                    for e in indices.iter_mut() {
                        self.expression(e, &shapes)?;
                    }
                    if let Some(dims) = shapes.get(name).filter(|d| d.len() > 1) {
                        self.indices(name, indices, dims)?;
                    }
                    self.expression(value, &shapes)?;
                }
                Stmt::Print { items }
                | Stmt::Read { args: items }
                | Stmt::CallSub { args: items, .. } => {
                    for e in items {
                        self.expression(e, &shapes)?;
                    }
                }
                Stmt::Return(Some(e)) => self.expression(e, &shapes)?,
                Stmt::If {
                    cond,
                    then_body,
                    else_body,
                } => {
                    self.expression(cond, &shapes)?;
                    self.body(then_body, &shapes)?;
                    if let Some(b) = else_body {
                        self.body(b, &shapes)?;
                    }
                }
                Stmt::Do {
                    start, end, body, ..
                } => {
                    self.expression(start, &shapes)?;
                    self.expression(end, &shapes)?;
                    self.body(body, &shapes)?;
                }
                Stmt::DoWhile { cond, body } => {
                    self.expression(cond, &shapes)?;
                    self.body(body, &shapes)?;
                }
                Stmt::Block { body }
                | Stmt::Function { body, .. }
                | Stmt::Subroutine { body, .. } => self.body(body, &shapes)?,
                Stmt::Module { body, .. } => {
                    let saved = std::mem::take(&mut self.types);
                    self.body(body, &Shapes::new())?;
                    self.types = saved;
                }
                Stmt::SelectCase {
                    expr,
                    cases,
                    default,
                } => {
                    self.expression(expr, &shapes)?;
                    for c in cases {
                        for item in &mut c.items {
                            match item {
                                CaseItem::Single(e) => self.expression(e, &shapes)?,
                                CaseItem::Range(a, b) => {
                                    self.expression(a, &shapes)?;
                                    self.expression(b, &shapes)?;
                                }
                            }
                        }
                        self.body(&mut c.body, &shapes)?;
                    }
                    if let Some(b) = default {
                        self.body(b, &shapes)?;
                    }
                }
                _ => {}
            }
        }
        self.types = inherited_types;
        Ok(())
    }
}
pub fn resolve(mut p: Program) -> Result<Program> {
    let mut lower = Lowerer::default();
    for s in &p.body {
        if let Stmt::Module { body, .. } = s {
            for s in body {
                if let Stmt::ArrayDecl { name, dims, .. } = s {
                    if dims.len() > 1 {
                        lower.module_arrays.insert(name.clone());
                    }
                }
            }
        }
    }
    fn functions(body: &[Stmt], out: &mut HashMap<String, TypeSpec>) {
        for s in body {
            match s {
                Stmt::Function {
                    name,
                    return_type,
                    body,
                    ..
                } => {
                    if let Some(kind) = return_type {
                        out.insert(name.clone(), kind.clone());
                    } else {
                        let result = match body.last() {
                            Some(Stmt::Return(Some(Expr::Ident(n)))) => n,
                            _ => name,
                        };
                        for s in body {
                            if let Stmt::VarDecl { names, kind } = s {
                                if names.contains(result) {
                                    out.insert(name.clone(), kind.clone());
                                }
                            }
                        }
                    }
                    functions(body, out);
                }
                Stmt::Module { body, .. }
                | Stmt::Subroutine { body, .. }
                | Stmt::Block { body } => functions(body, out),
                _ => {}
            }
        }
    }
    functions(&p.body, &mut lower.functions);
    lower.body(&mut p.body, &Shapes::new())?;
    p.body.extend(lower.guards);
    Ok(p)
}

#[cfg(test)]
mod tests {
    fn lower(source: &str) -> Result<Program> {
        let p = crate::parser::parse_with_src(source, &crate::lexer::lex(source), "matrix.f90")
            .unwrap();
        crate::array_calls::resolve(crate::parameters::resolve(crate::derived::resolve(p)?)?)
    }
    use super::*;
    #[test]
    fn rejects_invalid_matrix_shapes_and_subscripts() {
        for (source, expected) in [
            ("module m\ninteger::a(2,3)\nend module m\nuse m, only:a\nprint *,a\nend", "USE-associated multidimensional array"),
            ("integer,allocatable::a(:,:)\na=1\nallocate(a(2,2))\nend", "used before ALLOCATE"),
            ("integer::a(2,3)\ninteger(16)::i\ni=1\na=0\nprint *,a(i,1)\nend", "scalar INTEGER"),
            ("integer::a(2,3)\na=0\nprint *,a(f(),1)\ncontains\ninteger function f()\nf=1\nend function f\nend", "procedure calls"),
            (
                "integer,allocatable::a(:,:)\nallocate(a(2))\nend",
                "rank mismatch",
            ),
            (
                "integer,allocatable::a(:,:)\nallocate(a(2,0))\nend",
                "positive extent",
            ),
            (
                "integer,allocatable::a(:,:)\ninteger::n\nread(*,*) n\nallocate(a(n,2))\nend",
                "shape known",
            ),
            (
                "integer,allocatable::a(2,:)\nallocate(a(2,2))\nend",
                "all dimensions",
            ),
            (
                "integer::a(2,3),b(3,2)\na=0\nb=a\nend",
                "nonconforming array assignment",
            ),
            ("integer::a(2,3)\na=0\nprint *,a(3,1)\nend", "dimension 1"),
            ("integer::a(2,3)\na=0\nprint *,a(0,2)\nend", "dimension 1"),
            ("integer::a(2,3)\na=0\nprint *,a(1,4)\nend", "dimension 2"),
            (
                "integer::a(2,3)\na=0\nprint *,a(1)\nend",
                "requires 2 subscripts",
            ),
            (
                "integer::a(2,3)\nreal::r\na=0\nr=1.5\nprint *,a(r,1)\nend",
                "scalar INTEGER",
            ),
            ("integer::a(2,3)\nprint *,size(a,3)\nend", "SIZE dimension"),
            (
                "integer::a(2,3)\na=0\nprint *,a(:)\nend",
                "multidimensional sections",
            ),
        ] {
            let err = lower(source).unwrap_err().to_string();
            assert!(err.contains(expected), "{source}: {err}");
        }
    }
    #[test]
    fn matrices_preserve_column_major_storage_and_initialization_checks() {
        let p = lower(include_str!("../tests/66_allocatable_matrices.f90")).unwrap();
        assert!(p.body.iter().any(|s|matches!(s,Stmt::ArrayDecl{name,dims,..} if name=="a" && matches!(dims.as_slice(),[Expr::IntLit(n)] if n=="6"))));
        let p = lower("integer::a(2,3)\na(1,1)=1\nprint *,a(2,1)\nend").unwrap();
        let p = crate::sections::resolve(crate::reductions::resolve(p).unwrap()).unwrap();
        let err = crate::initialized::instrument(p).unwrap_err().to_string();
        assert!(err.contains("uninitialized variable `a`"), "{err}");
    }
}
