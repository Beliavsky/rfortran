"""Numeric allocatable arrays with runtime shape and initialization tracking."""
import ast
from values import Value


class AllocatableSupport:
    def declare_allocatable(self,node):
        typ,name,rank = node.data
        self.require(name not in self.symbols and name not in self.procedures,node.line,f"duplicate declaration `{name}`")
        self.require(typ in ("i32","f32","f64"),node.line,"only numeric allocatable arrays are currently supported")
        self.require(1 <= rank <= 15,node.line,"allocatable array rank must be between 1 and 15")
        self.allocatables[name] = rank
        self.symbols[name],self.arrays[name] = typ,-1
        self.needs_array_runtime = self.needs_alloc_runtime = True
        return f"let mut {self.identifier(name)}: Option<RfArray<{typ}>> = None;"

    def alloc_ref(self,name,line,mutable=False):
        method,borrow = ("rf_alloc_mut","&mut ") if mutable else ("rf_alloc_ref","&")
        return f'{method}({borrow}{self.rust_name(name)}, {line}, "{name}")'

    def alloc_value(self,name,line):
        ref = self.alloc_ref(name,line)
        return Value(f'{ref}.initialized_values({line}, "{name}")',self.symbols[name],-1,
                     rank=self.allocatables[name],shape=f"{ref}.shape.clone()")

    def alloc_inquiry(self,name,args,state,line):
        self.require(len(args) in ((1,) if name=="allocated" else (1,2)),line,f"unsupported {name.upper()} arguments")
        self.require(isinstance(args[0],ast.Name),line,f"{name.upper()} currently requires an array variable")
        target = args[0].id.removeprefix("_rf_name_")
        self.require(target in self.allocatables,line,f"{name.upper()} requires a supported allocatable array")
        if name=="allocated":
            return Value(f"{self.rust_name(target)}.is_some()","bool")
        ref = self.alloc_ref(target,line)
        if len(args)==1:
            return Value(f"({ref}.values.len() as i32)","i32")
        dim = self.expr(args[1],state,line)
        self.require(dim.typ=="i32" and dim.extent is None,line,"SIZE DIM must be a scalar default INTEGER")
        return Value(f'{ref}.size_dim({dim.code}, {line}, "{target}")',"i32")

    def allocate_statement(self,args,state,line,deallocate=False):
        self.require(bool(args),line,"allocation statement requires a destination")
        lines = []
        for arg in args:
            if deallocate:
                self.require(isinstance(arg,ast.Name),line,"DEALLOCATE requires array variables")
                name = arg.id.removeprefix("_rf_name_")
                self.require(name in self.allocatables,line,f"`{name}` is not a supported allocatable array")
                rust = self.rust_name(name)
                lines.append(f'rf_alloc_ref(&{rust}, {line}, "{name}"); {rust} = None;')
            else:
                self.require(isinstance(arg,ast.Call) and isinstance(arg.func,ast.Name),line,"ALLOCATE currently requires explicit extent lists")
                name = arg.func.id.removeprefix("_rf_name_")
                self.require(name in self.allocatables,line,f"`{name}` is not a supported allocatable array")
                self.require(len(arg.args)==self.allocatables[name],line,"ALLOCATE rank does not match declaration")
                dims = [self.expr(dim,state,line) for dim in arg.args]
                self.require(all(v.typ=="i32" and v.extent is None for v in dims),line,"allocation extents must be scalar default INTEGER")
                typ,rust = self.symbols[name],self.rust_name(name)
                zero = "0i32" if typ=="i32" else "0.0"+typ
                lines.append(f'rf_allocate(&mut {rust}, &[{", ".join(v.code for v in dims)}], {zero}, {line}, "{name}");')
        return lines

    def alloc_indices(self,name,args,state,line):
        self.require(len(args)==self.allocatables[name],line,"subscript rank does not match allocatable declaration")
        values = [self.expr(arg,state,line) for arg in args]
        self.require(all(v.typ=="i32" and v.extent is None for v in values),line,"array subscripts must be scalar default INTEGER")
        return "&["+", ".join(v.code for v in values)+"]"

    def alloc_element(self,name,args,state,line):
        indices = self.alloc_indices(name,args,state,line)
        return Value(f'{self.alloc_ref(name,line)}.element({indices}, {line}, "{name}")',self.symbols[name])

    def alloc_element_assignment(self,name,args,text,state,line):
        indices = self.alloc_indices(name,args,state,line)
        value = self.cast(self.expr(text,state,line),self.symbols[name],line)
        self.require(value.extent is None,line,"array element assignment requires a scalar")
        return ["{",f'    let rf_offset = {self.alloc_ref(name,line)}.offset({indices}, {line}, "{name}");',
                f"    let rf_element_value = {value.code};",f"    let rf_array = {self.alloc_ref(name,line,True)};",
                "    rf_array.values[rf_offset] = rf_element_value;", "    rf_array.initialized[rf_offset] = true;", "}"]

    def assign_allocatable(self,name,value,line):
        value = self.cast(value,self.symbols[name],line)
        if value.extent is None:
            return f'{{ let rf_value = {value.code}; let rf_array = {self.alloc_ref(name,line,True)}; rf_array.values.fill(rf_value); rf_array.initialized.fill(true); }}'
        self.require(value.rank==self.allocatables[name],line,"array assignment rank mismatch")
        shape = "vec![rf_values.len()]" if value.rank==1 else value.shape
        self.require(shape is not None,line,"array assignment requires shape metadata")
        code = value.code if value.owned else f"({value.code}).to_vec()"
        return f'{{ let rf_values = {code}; let rf_shape = {shape}; {self.rust_name(name)} = Some(RfArray {{ initialized: vec![true; rf_values.len()], values: rf_values, shape: rf_shape }}); }}'

    def alloc_random_number(self,name,line):
        typ = self.symbols[name]
        self.require(typ in ("f32","f64"),line,"RANDOM_NUMBER requires a real destination")
        return f'{{ let rf_array = {self.alloc_ref(name,line,True)}; for rf_value in &mut rf_array.values {{ *rf_value = rf_rng.next({str(typ=="f64").lower()}) as {typ}; }} rf_array.initialized.fill(true); }}'


ALLOC_RUNTIME = '''
struct RfArray<T> {
    values: Vec<T>,
    initialized: Vec<bool>,
    shape: Vec<usize>,
}

fn rf_alloc_ref<'a,T>(array: &'a Option<RfArray<T>>, line: usize, name: &str) -> &'a RfArray<T> {
    array.as_ref().unwrap_or_else(|| panic!("Fortran {}: array `{name}` is not allocated", rf_location(line)))
}

fn rf_alloc_mut<'a,T>(array: &'a mut Option<RfArray<T>>, line: usize, name: &str) -> &'a mut RfArray<T> {
    array.as_mut().unwrap_or_else(|| panic!("Fortran {}: array `{name}` is not allocated", rf_location(line)))
}

fn rf_allocate<T: Copy>(array: &mut Option<RfArray<T>>, extents: &[i32], zero: T, line: usize, name: &str) {
    assert!(array.is_none(), "Fortran {}: array `{name}` is already allocated", rf_location(line));
    let shape: Vec<usize> = extents.iter().map(|&n| n.max(0) as usize).collect();
    let count = if shape.contains(&0) { 0 } else {
        shape.iter().try_fold(1usize, |n, &dim| n.checked_mul(dim))
            .filter(|&n| n <= i32::MAX as usize)
            .unwrap_or_else(|| panic!("Fortran {}: allocation size exceeds default INTEGER range for `{name}`", rf_location(line)))
    };
    *array = Some(RfArray { values: vec![zero; count], initialized: vec![false; count], shape });
}

impl<T: Copy> RfArray<T> {
    fn initialized_values(&self, line: usize, name: &str) -> &[T] {
        rf_initialized(&self.values, &self.initialized, line, name)
    }

    fn size_dim(&self, dim: i32, line: usize, name: &str) -> i32 {
        assert!(dim >= 1 && dim as usize <= self.shape.len(),
                "Fortran {}: SIZE DIM {dim} is outside the rank of `{name}`", rf_location(line));
        self.shape[dim as usize - 1] as i32
    }

    fn offset(&self, indices: &[i32], line: usize, name: &str) -> usize {
        assert_eq!(indices.len(), self.shape.len(), "subscript rank mismatch");
        let (mut offset, mut stride) = (0usize, 1usize);
        for (&index, &extent) in indices.iter().zip(&self.shape) {
            assert!(index >= 1 && index as usize <= extent,
                    "Fortran {}: subscript {index} is outside 1..={extent} for `{name}`", rf_location(line));
        }
        for (&index, &extent) in indices.iter().zip(&self.shape) {
            offset += (index as usize - 1) * stride;
            stride *= extent;
        }
        offset
    }

    fn element(&self, indices: &[i32], line: usize, name: &str) -> T {
        let offset = self.offset(indices, line, name);
        assert!(self.initialized[offset], "Fortran {}: read of uninitialized array `{name}`: subscripts {indices:?}", rf_location(line));
        self.values[offset]
    }
}
'''
