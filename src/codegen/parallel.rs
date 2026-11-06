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

fn references(body: &[IStmt], out: &mut HashSet<String>, ints: &HashMap<String, Storage>) {
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
            // DCE/unrolling can leave literal stores to removed integer slots.
            // Ordinary codegen ignores them. A read is still collected below
            // and will fail the capture proof if its storage is unavailable.
            IStmt::AssignIntLit(n, _) if !ints.contains_key(n) => {}
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
                references(then_body, out, ints);
                if let Some(b) = else_body {
                    references(b, out, ints);
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
                references(body, out, ints);
            }
            IStmt::DoWhile { cond, body } => {
                expr(cond, out);
                references(body, out, ints);
            }
            IStmt::Block { body } => references(body, out, ints),
            IStmt::SelectCase {
                selector,
                cases,
                default,
            } => {
                expr(selector, out);
                for case in cases {
                    for item in &case.items {
                        match item {
                            crate::ir::IcCaseItem::Single(e) => expr(e, out),
                            crate::ir::IcCaseItem::Range(a, b) => {
                                expr(a, out);
                                expr(b, out);
                            }
                            crate::ir::IcCaseItem::Multi(items) => {
                                for e in items {
                                    expr(e, out);
                                }
                            }
                        }
                    }
                    references(&case.body, out, ints);
                }
                if let Some(body) = default {
                    references(body, out, ints);
                }
            }
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
    if let Err(error) = super::parallel_calls::check(body) {
        crate::concurrent_reduce::parallel::report(&format!("serial fallback: {error}"));
        return Ok(false);
    }
    let metadata = marker(body).unwrap_or_default();
    let mut outputs = Vec::new();
    let mut lengths = HashMap::new();
    let mut scratch = HashSet::new();
    let mut initialization_masks = Vec::new();
    if metadata.len() % 2 != 0 {
        bail!("invalid parallel reduction metadata");
    }
    for pair in metadata.chunks_exact(2) {
        if let IExpr::Call(tag, args) = &pair[0] {
            if tag == "@parallel_scratch" {
                if let Some(name) = args.first().and_then(storage_name) {
                    scratch.insert(name);
                    continue;
                }
                bail!("invalid parallel scratch metadata");
            }
            if tag == "@parallel_init" {
                if let [IExpr::Ident(value), IExpr::Ident(flag)] = args.as_slice() {
                    initialization_masks.push((value.clone(), flag.clone(), pair[1].clone()));
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
        if let IExpr::Call(tag, args) = &pair[0] {
            if tag == "@packed_view" && args.len() == 2 {
                lengths.insert(value.clone(), args[1].clone());
            }
        }
        outputs.push((value, array));
    }
    let mut used = HashSet::new();
    for (value, array) in &outputs {
        used.insert(value.clone());
        used.insert(array.clone());
    }
    references(body, &mut used, ints);
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
                IStmt::SelectCase { cases, default, .. } => {
                    for case in cases {
                        private_declarations(&case.body, used, private);
                    }
                    if let Some(body) = default {
                        private_declarations(body, used, private);
                    }
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
    // Buffer ownership is recorded before later passes hoist declarations.
    // A caller's section buffer passed as an actual argument remains shared.
    private_names.extend(scratch.iter().cloned());
    let reduction_masks = initialization_masks
        .iter()
        .filter_map(|(n, flag, _)| outputs.iter().any(|(v, _)| v == n).then_some(flag.clone()))
        .collect::<HashSet<_>>();
    let private_masks = initialization_masks
        .into_iter()
        .filter_map(|(n, flag, length)| {
            if private_names.contains(&n) {
                if !matches!(&length,IExpr::IntLit(n) if n=="0") {
                    lengths.insert(flag.clone(), length);
                }
                Some(flag)
            } else {
                None
            }
        })
        .collect::<HashSet<_>>();
    // Generated heap temporaries are bound and allocated in the iteration.
    // Their masks must start with empty private descriptors, rather than
    // copying caller storage whose size has not been computed yet. Cleanup
    // inserted by heap::cleanup frees both data and masks at lexical exits.
    fn heap_bindings(body: &[IStmt], bound: &mut HashSet<String>) {
        for stmt in body {
            match stmt {
                IStmt::Call(tag, args) if tag == "@heap_bind" => {
                    if let Some(IExpr::Ident(n)) = args.first() {
                        bound.insert(n.clone());
                    }
                }
                IStmt::Do { body, .. } | IStmt::DoWhile { body, .. } | IStmt::Block { body } => {
                    heap_bindings(body, bound)
                }
                IStmt::If {
                    then_body,
                    else_body,
                    ..
                } => {
                    heap_bindings(then_body, bound);
                    if let Some(body) = else_body {
                        heap_bindings(body, bound);
                    }
                }
                IStmt::SelectCase { cases, default, .. } => {
                    for case in cases {
                        heap_bindings(&case.body, bound);
                    }
                    if let Some(body) = default {
                        heap_bindings(body, bound);
                    }
                }
                _ => {}
            }
        }
    }
    let mut bound_heaps = HashSet::new();
    heap_bindings(body, &mut bound_heaps);
    let mut worker_heaps = vec![];
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
        if bound_heaps.contains(&n) && (private_names.contains(&n) || private_masks.contains(&n)) {
            // The declaration may already be hoisted into caller storage, or
            // may still be a placeholder until @heap_bind runs in the worker.
            worker_heaps.push((n, kind));
            continue;
        }
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
        if scratch.contains(&n) && matches!(storage, Storage::HeapStack(_) | Storage::HeapStatic(_))
        {
            crate::concurrent_reduce::parallel::report(
                "serial fallback: runtime-sized section scratch storage",
            );
            return Ok(false);
        }
        if matches!(storage, Storage::HeapStack(_) | Storage::HeapStatic(_))
            && (outputs.iter().any(|(v, _)| v == &n) || private_masks.contains(&n))
            && !lengths.contains_key(&n)
        {
            crate::concurrent_reduce::parallel::report(
                "serial fallback: missing private array extent",
            );
            return Ok(false);
        }
        captures.push((n, storage, kind));
    }
    // Byte lengths are evaluated in the caller, where runtime descriptors live.
    // Appended capture slots keep this independent of scalar capture ordering.
    let private_lengths = captures
        .iter()
        .filter_map(|(n, _, _)| {
            if outputs.iter().any(|(v, _)| v == n)
                || private_masks.contains(n)
                || scratch.contains(n)
            {
                arrays.get(n).map(|count| {
                    (
                        n.clone(),
                        lengths
                            .get(n)
                            .cloned()
                            .unwrap_or(IExpr::IntLit(count.to_string())),
                    )
                })
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    fn assigned_scalars(body: &[IStmt], assigned: &mut HashSet<String>) {
        for stmt in body {
            match stmt {
                IStmt::AssignExpr(n, _)
                | IStmt::AssignIdent(n, _)
                | IStmt::AssignIntLit(n, _)
                | IStmt::AssignRealLit(n, _)
                | IStmt::AssignBool(n, _)
                | IStmt::AssignStr(n, _) => {
                    assigned.insert(n.clone());
                }
                IStmt::Do { var, body, .. } => {
                    assigned.insert(var.clone());
                    assigned_scalars(body, assigned);
                }
                IStmt::DoWhile { body, .. } | IStmt::Block { body } => {
                    assigned_scalars(body, assigned)
                }
                IStmt::If {
                    then_body,
                    else_body,
                    ..
                } => {
                    assigned_scalars(then_body, assigned);
                    if let Some(body) = else_body {
                        assigned_scalars(body, assigned);
                    }
                }
                IStmt::SelectCase { cases, default, .. } => {
                    for case in cases {
                        assigned_scalars(&case.body, assigned);
                    }
                    if let Some(body) = default {
                        assigned_scalars(body, assigned);
                    }
                }
                _ => {}
            }
        }
    }
    let mut assigned = HashSet::new();
    assigned_scalars(body, &mut assigned);
    for (name, length) in &private_lengths {
        let mut inputs = HashSet::new();
        references(
            &[IStmt::AssignExpr(String::new(), length.clone())],
            &mut inputs,
            ints,
        );
        inputs.remove("");
        if inputs.iter().any(|n| {
            (assigned.contains(n)
                && !outputs.iter().any(|(value, _)| value == name)
                && !reduction_masks.contains(name))
                || (!ints.contains_key(n) && !reals.contains_key(n) && !bools.contains_key(n))
        }) {
            crate::concurrent_reduce::parallel::report(
                "serial fallback: private array extent depends on worker-local storage",
            );
            return Ok(false);
        }
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
        for (n, kind) in &worker_heaps {
            let cell = w.create_sized_stack_slot(ir::StackSlotData::new(
                ir::StackSlotKind::ExplicitSlot,
                8,
                0,
            ));
            let zero = w.ins().iconst(ptr, 0);
            w.ins().stack_store(zero, cell, 0);
            let storage = Storage::HeapStack(cell);
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
            wa.insert(n.clone(), 1);
        }
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
                let private = outputs.iter().any(|(value, _)| value == n)
                    || private_masks.contains(n)
                    || scratch.contains(n);
                let address = if private {
                    let position = private_lengths
                        .iter()
                        .position(|(name, _)| name == n)
                        .unwrap();
                    let bytes = w.ins().load(
                        ptr,
                        ir::MemFlags::new(),
                        params[0],
                        ((captures.len() + position) * 8) as i32,
                    );
                    let address =
                        if matches!(original, Storage::HeapStack(_) | Storage::HeapStatic(_)) {
                            w.ins().load(ptr, ir::MemFlags::new(), address, 0)
                        } else {
                            address
                        };
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
                let heap = matches!(original, Storage::HeapStack(_) | Storage::HeapStatic(_));
                let address = if private && heap {
                    let descriptor = w.create_sized_stack_slot(ir::StackSlotData::new(
                        ir::StackSlotKind::ExplicitSlot,
                        8,
                        0,
                    ));
                    w.ins().stack_store(address, descriptor, 0);
                    w.ins().stack_addr(ptr, descriptor, 0)
                } else {
                    address
                };
                w.ins().stack_store(address, slot, 0);
                Storage::Borrowed {
                    cell: slot,
                    original: BorrowedOriginal::Stack(slot),
                    heap,
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
            let bytes = if length.is_some() {
                let position = private_lengths
                    .iter()
                    .position(|(name, _)| name == value)
                    .unwrap();
                w.ins().load(
                    ptr,
                    ir::MemFlags::new(),
                    params[0],
                    ((captures.len() + position) * 8) as i32,
                )
            } else {
                w.ins().iconst(ptr, 8)
            };
            let offset = w.ins().imul(params[3], bytes);
            let address = w.ins().iadd(address, offset);
            if length.is_some() {
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
        ((captures.len() + private_lengths.len()).max(1) * 8) as u32,
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
    for (i, (_, length)) in private_lengths.iter().enumerate() {
        let length = eval_expr(b, ex, ptr, length, ints, reals, bools, func_meta);
        let length = to_i64(b, length);
        let bytes = b.ins().imul_imm(length, 8);
        b.ins()
            .stack_store(bytes, record, ((captures.len() + i) * 8) as i32);
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
    #[test]
    fn parallel_capture_reads_survive_dead_integer_store_filter() {
        let ints = HashMap::new();
        let mut used = HashSet::new();
        references(
            &[IStmt::AssignIntLit("@reduction_index_1".into(), "5".into())],
            &mut used,
            &ints,
        );
        assert!(used.is_empty());
        references(
            &[IStmt::SelectCase {
                selector: IExpr::IntLit("1".into()),
                cases: vec![],
                default: Some(vec![IStmt::AssignExpr(
                    "result".into(),
                    IExpr::Ident("@reduction_index_1".into()),
                )]),
            }],
            &mut used,
            &ints,
        );
        assert!(used.contains("@reduction_index_1"));
        assert!(used.contains("result"));
    }
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
                                IExpr::Call(
                                    "scalar_worker_helper".into(),
                                    vec![IExpr::Ident("i".into())],
                                ),
                            ),
                        ],
                    },
                ],
            }],
        };
        let mut module = module;
        module.funcs.push(crate::ir::Function {
            name: "scalar_worker_helper".into(),
            params: vec!["x".into()],
            return_type: Some(TypeSpec::Real),
            body: vec![
                IStmt::DeclVar(TypeSpec::Integer(None), "x".into(), vec![]),
                IStmt::Return(Some(IExpr::Call(
                    "real".into(),
                    vec![IExpr::Ident("x".into())],
                ))),
            ],
        });
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
                if heap && array {
                    module.funcs[0].body.insert(
                        0,
                        IStmt::DeclVar(TypeSpec::Integer(Some(8)), "runtime_count".into(), vec![]),
                    );
                    module.funcs[0]
                        .body
                        .insert(1, IStmt::AssignIntLit("runtime_count".into(), "3".into()));
                    let position = module.funcs[0].body.len() - 1;
                    module.funcs[0].body.insert(
                        position,
                        IStmt::Call("@heap_bind".into(), vec![IExpr::Ident("s".into())]),
                    );
                    if let Some(IStmt::Do { body, .. }) = module.funcs[0].body.last_mut() {
                        if let IStmt::Call(_, args) = &mut body[0] {
                            args[0] = IExpr::Call(
                                "@packed_view".into(),
                                vec![
                                    IExpr::Ident("s".into()),
                                    IExpr::Ident("runtime_count".into()),
                                ],
                            );
                        }
                    }
                }
                if heap && array {
                    let ident = |n: &str| IExpr::Ident(n.into());
                    let call = |n: &str, args| IStmt::Call(n.into(), args);
                    let position = module.funcs[0].body.len() - 1;
                    module.funcs[0].body.splice(
                        position..position,
                        [
                            IStmt::DeclVar(
                                TypeSpec::Real,
                                "dynamic_scratch".into(),
                                vec![IExpr::IntLit("1".into())],
                            ),
                            IStmt::DeclVar(
                                TypeSpec::Integer(Some(8)),
                                "dynamic_mask".into(),
                                vec![IExpr::IntLit("1".into())],
                            ),
                            call("@heap_bind", vec![ident("dynamic_scratch")]),
                            call("@heap_bind", vec![ident("dynamic_mask")]),
                        ],
                    );
                    if let Some(IStmt::Do { body, .. }) = module.funcs[0].body.last_mut() {
                        if let IStmt::Call(_, args) = &mut body[0] {
                            args.extend([
                                IExpr::Call(
                                    "@parallel_scratch".into(),
                                    vec![ident("dynamic_scratch")],
                                ),
                                IExpr::IntLit("0".into()),
                                IExpr::Call(
                                    "@parallel_init".into(),
                                    vec![ident("dynamic_scratch"), ident("dynamic_mask")],
                                ),
                                ident("dynamic_length"),
                            ]);
                        }
                        body.extend([
                            IStmt::DeclVar(
                                TypeSpec::Integer(Some(8)),
                                "dynamic_length".into(),
                                vec![],
                            ),
                            call("@heap_bind", vec![ident("dynamic_scratch")]),
                            call("@heap_bind", vec![ident("dynamic_mask")]),
                            call(
                                "@heap_allocate",
                                vec![
                                    ident("dynamic_scratch"),
                                    ident("dynamic_length"),
                                    ident("dynamic_mask"),
                                    IExpr::IntLit("3".into()),
                                ],
                            ),
                            IStmt::AssignIndex(
                                "dynamic_scratch".into(),
                                vec![IExpr::IntLit("1".into())],
                                IExpr::RealLit("2.0".into()),
                            ),
                            call(
                                "@heap_drop",
                                vec![ident("dynamic_scratch"), ident("dynamic_mask")],
                            ),
                        ]);
                    }
                }
                if heap {
                    let position = module.funcs[0]
                        .body
                        .iter()
                        .position(|s| matches!(s,IStmt::DeclVar(_,n,_) if n=="a"))
                        .unwrap()
                        + 1;
                    module.funcs[0].body.insert(
                        position,
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
                assert!(
                    object
                        .windows(b"rfort.parallel.".len())
                        .any(|s| s == b"rfort.parallel."),
                    "worker missing on {target}, heap={heap}, array={array}"
                );
            }
        }
    }
}
