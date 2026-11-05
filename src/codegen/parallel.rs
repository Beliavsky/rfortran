// SPDX-License-Identifier: MIT
//! Outline a proven loop as a native callback with an explicit capture record.
use super::*;

// Array lowering may wrap a loop body in lexical blocks. Do not search
// nested loops or conditional branches: their markers belong to other loops.
pub(super) fn marker(body: &[IStmt]) -> Option<&[IExpr]> {
    body.iter().find_map(|s| match s {
        IStmt::Call(n, args) if n == "@parallel_iteration" => Some(args.as_slice()),
        IStmt::Block { body } => marker(body),
        _ => None,
    })
}

fn references(body: &[IStmt], out: &mut HashSet<String>) {
    fn expr(e: &IExpr, out: &mut HashSet<String>) {
        match e {
            IExpr::Ident(n) => {
                out.insert(n.clone());
            }
            IExpr::Index(n, args) => {
                out.insert(n.clone());
                for a in args {
                    expr(a, out);
                }
            }
            IExpr::Call(_, args) => {
                for a in args {
                    expr(a, out);
                }
            }
            IExpr::Bin(_, a, b) => {
                expr(a, out);
                expr(b, out);
            }
            IExpr::Un(_, a) => expr(a, out),
            _ => {}
        }
    }
    for s in body {
        match s {
            IStmt::DeclVar(_, n, d) => {
                out.insert(n.clone());
                for e in d {
                    expr(e, out);
                }
            }
            IStmt::AssignStr(n, _)
            | IStmt::AssignIntLit(n, _)
            | IStmt::AssignRealLit(n, _)
            | IStmt::AssignBool(n, _) => {
                out.insert(n.clone());
            }
            IStmt::AssignIdent(n, v) => {
                out.insert(n.clone());
                out.insert(v.clone());
            }
            IStmt::AssignExpr(n, e) => {
                out.insert(n.clone());
                expr(e, out);
            }
            IStmt::AssignIndex(n, args, e) => {
                out.insert(n.clone());
                for a in args {
                    expr(a, out);
                }
                expr(e, out);
            }
            IStmt::If {
                cond,
                then_body,
                else_body,
            } => {
                expr(cond, out);
                references(then_body, out);
                if let Some(b) = else_body {
                    references(b, out);
                }
            }
            IStmt::Do {
                var,
                start,
                end,
                body,
            } => {
                out.insert(var.clone());
                expr(start, out);
                expr(end, out);
                references(body, out);
            }
            IStmt::DoWhile { cond, body } => {
                expr(cond, out);
                references(body, out);
            }
            IStmt::Block { body } => references(body, out),
            IStmt::Call(n, _) if n == "@parallel_iteration" => {}
            IStmt::Call(_, args) | IStmt::Print(args) | IStmt::Read(args) => {
                for a in args {
                    expr(a, out);
                }
            }
            IStmt::Return(Some(e)) => expr(e, out),
            _ => {}
        }
    }
}
#[allow(clippy::too_many_arguments)]
pub(super) fn emit(
    b: &mut FunctionBuilder,
    ex: &Externs,
    ptr: ir::Type,
    module: &mut ObjectModule,
    var: &str,
    start: &IExpr,
    end: &IExpr,
    body: &[IStmt],
    ints: &HashMap<String, Storage>,
    reals: &HashMap<String, Storage>,
    bools: &HashMap<String, Storage>,
    real_kind: &HashMap<String, bool>,
    arrays: &HashMap<String, usize>,
    func_meta: &HashMap<String, FuncMeta>,
    tokens: Option<&[crate::lexer::Token]>,
) -> Result<bool> {
    fn storage_name(e: &IExpr) -> Option<String> {
        match e {
            IExpr::Ident(n) | IExpr::Str(n) => Some(n.clone()),
            IExpr::Call(tag, args) if tag == "@packed_view" => args.first().and_then(storage_name),
            _ => None,
        }
    }
    let metadata = marker(body).unwrap_or_default();
    let mut outputs = Vec::new();
    let mut initialization_masks = Vec::new();
    if metadata.len() % 2 != 0 {
        bail!("invalid parallel reduction metadata");
    }
    for pair in metadata.chunks_exact(2) {
        if let IExpr::Call(tag, args) = &pair[0] {
            if tag == "@parallel_init" {
                if let [IExpr::Ident(value), IExpr::Ident(flag)] = args.as_slice() {
                    initialization_masks.push((value.clone(), flag.clone()));
                    continue;
                }
                bail!("invalid parallel initialization metadata");
            }
        }
        let (Some(value), Some(array)) = (storage_name(&pair[0]), storage_name(&pair[1])) else {
            crate::concurrent_reduce::parallel::report(
                "serial fallback: unsupported reduction storage view",
            );
            return Ok(false);
        };
        outputs.push((value, array));
    }
    let mut used = HashSet::new();
    for (value, array) in &outputs {
        used.insert(value.clone());
        used.insert(array.clone());
    }
    references(body, &mut used);
    // Later lowering can introduce per-iteration temporaries (for example
    // kind-qualified integer literals). Their declarations belong to the
    // callback, not to the caller's capture record.
    fn private_declarations(
        body: &[IStmt],
        used: &mut HashSet<String>,
        private: &mut HashSet<String>,
    ) {
        for s in body {
            match s {
                IStmt::DeclVar(_, n, _) => {
                    used.remove(n);
                    private.insert(n.clone());
                }
                IStmt::If {
                    then_body,
                    else_body,
                    ..
                } => {
                    private_declarations(then_body, used, private);
                    if let Some(body) = else_body {
                        private_declarations(body, used, private);
                    }
                }
                IStmt::Do { body, .. } | IStmt::DoWhile { body, .. } | IStmt::Block { body } => {
                    private_declarations(body, used, private)
                }
                _ => {}
            }
        }
    }
    let mut private_names = outputs
        .iter()
        .map(|(n, _)| n.clone())
        .collect::<HashSet<_>>();
    private_declarations(body, &mut used, &mut private_names);
    let private_masks = initialization_masks
        .into_iter()
        .filter_map(|(n, flag)| private_names.contains(&n).then_some(flag))
        .collect::<HashSet<_>>();
    used.insert(var.into());
    let mut names = used.into_iter().collect::<Vec<_>>();
    names.sort();
    // Source analysis excludes aliased storage and descriptor mutation. Scalar captures
    // become private copies; source analysis forbids writes to shared scalars.
    let mut captures = vec![];
    for n in names {
        let item = ints
            .get(&n)
            .map(|s| (*s, 0))
            .or_else(|| reals.get(&n).map(|s| (*s, 1)))
            .or_else(|| bools.get(&n).map(|s| (*s, 2)));
        let Some((storage, kind)) = item else {
            crate::concurrent_reduce::parallel::report(&format!(
                "serial fallback: unsupported lowered capture `{n}`"
            ));
            return Ok(false);
        };
        if !matches!(
            storage,
            Storage::Stack(_) | Storage::Static(_) | Storage::HeapStack(_) | Storage::HeapStatic(_)
        ) {
            crate::concurrent_reduce::parallel::report("serial fallback: aliased storage");
            return Ok(false);
        }
        if matches!(storage, Storage::HeapStack(_) | Storage::HeapStatic(_))
            && !arrays.contains_key(&n)
        {
            crate::concurrent_reduce::parallel::report(
                "serial fallback: unsupported heap scalar capture",
            );
            return Ok(false);
        }
        captures.push((n, storage, kind));
    }
    let mut context = module.make_context();
    context.func.signature.params.extend([
        ir::AbiParam::new(ptr),
        ir::AbiParam::new(ir::types::I64),
        ir::AbiParam::new(ir::types::I64),
        ir::AbiParam::new(ir::types::I64),
    ]);
    let name = format!(
        ".rfort.parallel.{}",
        GLOBAL_STR_ID.fetch_add(1, Ordering::Relaxed)
    );
    let fid = module.declare_function(&name, Linkage::Local, &context.func.signature)?;
    let mut fb = FunctionBuilderContext::new();
    // SSA values and variables belong to one function, never to its caller.
    let counters = LOOP_COUNTERS.with(|c| std::mem::take(&mut *c.borrow_mut()));
    let generated = (|| -> Result<()> {
        let mut w = FunctionBuilder::new(&mut context.func, &mut fb);
        let entry = w.create_block();
        w.append_block_params_for_function_params(entry);
        w.switch_to_block(entry);
        w.seal_block(entry);
        let params = w.block_params(entry).to_vec();
        let wx = declare_externs(module, &mut w, ptr)?;
        let mut wm = HashMap::new();
        for (name, meta) in func_meta {
            if let Some(cranelift_module::FuncOrDataId::Func(id)) = module.get_name(name) {
                wm.insert(
                    name.clone(),
                    FuncMeta {
                        fref: module.declare_func_in_func(id, w.func),
                        ret: meta.ret,
                        param_kinds: meta.param_kinds.clone(),
                        double: meta.double,
                    },
                );
            }
        }
        let mut wi = HashMap::new();
        let mut wr = HashMap::new();
        let mut wb = HashMap::new();
        let mut wa = HashMap::new();
        let mut private_arrays = vec![];
        for (i, (n, original, kind)) in captures.iter().enumerate() {
            let address = w
                .ins()
                .load(ptr, ir::MemFlags::new(), params[0], (i * 8) as i32);
            let slot = w.create_sized_stack_slot(ir::StackSlotData::new(
                ir::StackSlotKind::ExplicitSlot,
                8,
                0,
            ));
            let storage = if let Some(length) = arrays.get(n) {
                wa.insert(n.clone(), *length);
                let private =
                    outputs.iter().any(|(value, _)| value == n) || private_masks.contains(n);
                let address = if private {
                    let bytes = w.ins().iconst(ptr, (*length * 8) as i64);
                    let data = file_io::call(module, &mut w, "malloc", &[bytes], ptr)?;
                    let okay = w.ins().icmp_imm(ir::condcodes::IntCC::NotEqual, data, 0);
                    file_io::require(
                        module,
                        &mut w,
                        ptr,
                        &mut 0,
                        okay,
                        "parallel reduction allocation failed",
                    )?;
                    file_io::call(module, &mut w, "memcpy", &[data, address, bytes], ptr)?;
                    private_arrays.push(data);
                    data
                } else {
                    address
                };
                w.ins().stack_store(address, slot, 0);
                Storage::Borrowed {
                    cell: slot,
                    original: BorrowedOriginal::Stack(slot),
                    heap: !private
                        && matches!(original, Storage::HeapStack(_) | Storage::HeapStatic(_)),
                }
            } else {
                let ty = match kind {
                    1 => ir::types::F64,
                    2 => ir::types::I8,
                    _ => ir::types::I64,
                };
                let value = w.ins().load(ty, ir::MemFlags::new(), address, 0);
                w.ins().stack_store(value, slot, 0);
                Storage::Stack(slot)
            };
            match kind {
                1 => {
                    wr.insert(n.clone(), storage);
                }
                2 => {
                    wb.insert(n.clone(), storage);
                }
                _ => {
                    wi.insert(n.clone(), storage);
                }
            }
        }
        for (n, value) in [("@parallel_start", params[1]), ("@parallel_end", params[2])] {
            let slot = w.create_sized_stack_slot(ir::StackSlotData::new(
                ir::StackSlotKind::ExplicitSlot,
                8,
                0,
            ));
            w.ins().stack_store(value, slot, 0);
            wi.insert(n.into(), slot.into());
        }
        fn worker_body(body: &[IStmt]) -> Vec<IStmt> {
            body.iter()
                .filter_map(|s| match s {
                    IStmt::Call(n, _) if n == "@parallel_iteration" => None,
                    IStmt::Block { body } => Some(IStmt::Block {
                        body: worker_body(body),
                    }),
                    _ => Some(s.clone()),
                })
                .collect()
        }
        let body = worker_body(body);
        let loop_stmt = IStmt::Do {
            var: var.into(),
            start: IExpr::Ident("@parallel_start".into()),
            end: IExpr::Ident("@parallel_end".into()),
            body,
        };
        gen_stmts(
            &mut w,
            &wx,
            ptr,
            module,
            &mut 0,
            &[loop_stmt],
            &mut wi,
            &mut wr,
            &mut wb,
            &mut characters::Characters::new(),
            &mut real_kind.clone(),
            &mut HashSet::new(),
            &mut HashMap::new(),
            &wm,
            Some(&name),
            None,
            None,
            &mut wa,
            tokens,
        )?;
        // Each worker owns one slot. The synchronous dispatch joins before
        // the source-level fold reads these slots in worker-index order.
        for (value, array) in &outputs {
            let (storage, ty) = if let Some(s) = wi.get(value) {
                (*s, ir::types::I64)
            } else if let Some(s) = wr.get(value) {
                (*s, ir::types::F64)
            } else {
                (wb[value], ir::types::I8)
            };
            let source = storage_addr(&mut w, ptr, storage);
            let length = wa.get(value).copied();
            let destination = wi
                .get(array)
                .or_else(|| wr.get(array))
                .or_else(|| wb.get(array))
                .unwrap();
            let address = storage_addr(&mut w, ptr, *destination);
            let offset = w
                .ins()
                .imul_imm(params[3], (length.unwrap_or(1) * 8) as i64);
            let address = w.ins().iadd(address, offset);
            if let Some(length) = length {
                let bytes = w.ins().iconst(ptr, (length * 8) as i64);
                file_io::call(module, &mut w, "memcpy", &[address, source, bytes], ptr)?;
            } else {
                let result = w.ins().load(ty, ir::MemFlags::new(), source, 0);
                w.ins().store(ir::MemFlags::new(), result, address, 0);
            }
        }
        if !private_arrays.is_empty() {
            let mut sig = module.make_signature();
            sig.params.push(ir::AbiParam::new(ptr));
            let free = module.declare_function("free", Linkage::Import, &sig)?;
            let free = module.declare_func_in_func(free, w.func);
            for data in private_arrays {
                w.ins().call(free, &[data]);
            }
        }
        w.ins().return_(&[]);
        w.seal_all_blocks();
        w.finalize();
        Ok(())
    })();
    LOOP_COUNTERS.with(|c| *c.borrow_mut() = counters);
    generated?;
    large_functions::define(module, fid, &mut context)?;
    let record = b.create_sized_stack_slot(ir::StackSlotData::new(
        ir::StackSlotKind::ExplicitSlot,
        (captures.len().max(1) * 8) as u32,
        3,
    ));
    for (i, (_, storage, _)) in captures.iter().enumerate() {
        let address = if matches!(storage, Storage::HeapStack(_) | Storage::HeapStatic(_)) {
            heap_cell(b, ptr, *storage)?
        } else {
            storage_addr(b, ptr, *storage)
        };
        b.ins().stack_store(address, record, (i * 8) as i32);
    }
    let record = b.ins().stack_addr(ptr, record, 0);
    let reference = module.declare_func_in_func(fid, b.func);
    let callback = b.ins().func_addr(ptr, reference);
    let first = eval_expr(b, ex, ptr, start, ints, reals, bools, func_meta);
    let first = to_i64(b, first);
    let last = eval_expr(b, ex, ptr, end, ints, reals, bools, func_meta);
    let last = to_i64(b, last);
    file_io::call(
        module,
        b,
        "rfort_parallel_for",
        &[callback, record, first, last],
        ir::types::I32,
    )?;
    crate::concurrent_reduce::parallel::report(
        "native parallel worker emitted (small ranges run serially)",
    );
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn outlined_worker_objects_on_release_targets() {
        let module = crate::ir::Module {
            uses_modules: vec![],
            funcs: vec![crate::ir::Function {
                name: "main".into(),
                params: vec![],
                return_type: None,
                body: vec![
                    IStmt::DeclVar(TypeSpec::Integer(None), "i".into(), vec![]),
                    IStmt::DeclVar(
                        TypeSpec::Real,
                        "a".into(),
                        vec![IExpr::IntLit("10000".into())],
                    ),
                    IStmt::DeclVar(TypeSpec::Integer(None), "s".into(), vec![]),
                    IStmt::AssignIntLit("s".into(), "0".into()),
                    IStmt::DeclVar(
                        TypeSpec::Integer(None),
                        "partial".into(),
                        vec![IExpr::IntLit("64".into())],
                    ),
                    IStmt::Do {
                        var: "i".into(),
                        start: IExpr::IntLit("1".into()),
                        end: IExpr::IntLit("10000".into()),
                        body: vec![
                            IStmt::Call(
                                "@parallel_iteration".into(),
                                vec![IExpr::Str("s".into()), IExpr::Str("partial".into())],
                            ),
                            IStmt::AssignExpr(
                                "s".into(),
                                IExpr::Bin(
                                    crate::ast::BinOp::Add,
                                    Box::new(IExpr::Ident("s".into())),
                                    Box::new(IExpr::Ident("i".into())),
                                ),
                            ),
                            IStmt::AssignIndex(
                                "a".into(),
                                vec![IExpr::Ident("i".into())],
                                IExpr::RealLit("2.0".into()),
                            ),
                        ],
                    },
                ],
            }],
        };
        for target in [
            "x86_64-pc-windows-msvc",
            "x86_64-unknown-linux-gnu",
            "aarch64-apple-darwin",
        ] {
            for (heap, array) in [(false, false), (true, false), (false, true), (true, true)] {
                let mut module = module.clone();
                if array {
                    for stmt in &mut module.funcs[0].body {
                        match stmt {
                            IStmt::DeclVar(_, n, dims) if n == "s" => {
                                *dims = vec![IExpr::IntLit("3".into())]
                            }
                            IStmt::DeclVar(_, n, dims) if n == "partial" => {
                                *dims = vec![IExpr::IntLit("192".into())]
                            }
                            IStmt::AssignIntLit(n, _) if n == "s" => {
                                *stmt = IStmt::AssignIndex(
                                    "s".into(),
                                    vec![IExpr::IntLit("1".into())],
                                    IExpr::IntLit("0".into()),
                                )
                            }
                            IStmt::Do { body, .. } => {
                                body[0] = IStmt::Call(
                                    "@parallel_iteration".into(),
                                    vec![
                                        IExpr::Ident("s".into()),
                                        IExpr::Call(
                                            "@packed_view".into(),
                                            vec![
                                                IExpr::Ident("partial".into()),
                                                IExpr::IntLit("192".into()),
                                            ],
                                        ),
                                    ],
                                );
                                body[1] = IStmt::AssignIndex(
                                    "s".into(),
                                    vec![IExpr::IntLit("1".into())],
                                    IExpr::Bin(
                                        crate::ast::BinOp::Add,
                                        Box::new(IExpr::Index(
                                            "s".into(),
                                            vec![IExpr::IntLit("1".into())],
                                        )),
                                        Box::new(IExpr::Ident("i".into())),
                                    ),
                                );
                            }
                            _ => {}
                        }
                    }
                }
                if array {
                    let position = module.funcs[0].body.len() - 1;
                    module.funcs[0].body.insert(
                        position,
                        IStmt::DeclVar(
                            TypeSpec::Integer(None),
                            "scratch_mask".into(),
                            vec![IExpr::IntLit("2".into())],
                        ),
                    );
                    if let Some(IStmt::Do { body, .. }) = module.funcs[0].body.last_mut() {
                        if let IStmt::Call(_, args) = &mut body[0] {
                            args.extend([
                                IExpr::Call(
                                    "@parallel_init".into(),
                                    vec![
                                        IExpr::Ident("scratch".into()),
                                        IExpr::Ident("scratch_mask".into()),
                                    ],
                                ),
                                IExpr::IntLit("0".into()),
                            ]);
                        }
                        body.push(IStmt::DeclVar(
                            TypeSpec::Integer(None),
                            "scratch".into(),
                            vec![IExpr::IntLit("2".into())],
                        ));
                        body.push(IStmt::AssignIndex(
                            "scratch_mask".into(),
                            vec![IExpr::IntLit("1".into())],
                            IExpr::IntLit("1".into()),
                        ));
                    }
                }
                if heap {
                    module.funcs[0].body.insert(
                        2,
                        IStmt::Call("@heap_bind".into(), vec![IExpr::Ident("a".into())]),
                    );
                    if let Some(IStmt::Do { body, .. }) = module.funcs[0].body.last_mut() {
                        *body = vec![IStmt::Block {
                            body: std::mem::take(body),
                        }];
                    }
                }
                let mut backend = CraneliftBackend::default();
                backend.triple = Some(target.parse().unwrap());
                let object = backend.compile(&module, None).unwrap();
                assert!(!object.is_empty(), "{target}, heap={heap}");
            }
        }
    }
}
