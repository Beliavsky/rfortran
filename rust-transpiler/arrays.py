"""Rank-one numeric arrays and reductions for the partial Rust backend."""
import ast
from compiler_frontend import Unsupported
from rust_style import integer_constant
from values import Value, iterator


class ArraySupport:
    def declare_array(self,node,state):
        typ,name,extent = node.data
        self.require(name not in self.symbols,node.line,f"duplicate declaration `{name}`")
        self.require(typ in ("i32","f32","f64"),node.line,"only numeric arrays are currently supported")
        size = integer_constant(extent,self.plan.constants)
        self.require(size is not None,node.line,"array extent must currently be a constant default INTEGER expression")
        # Validate declaration-order and PARAMETER dependencies, not just the plan.
        dimension = self.expr(extent,state,node.line,constant=True)
        self.require(dimension.typ == "i32" and dimension.extent is None,node.line,"array extent must be a scalar INTEGER")
        size = max(0,size)
        self.symbols[name],self.arrays[name] = typ,size
        self.identifier(name)
        self.needs_array_runtime = True
        if not size:
            state.add(name)
        rust = self.rust_name(name)
        zero = "0i32" if typ=="i32" else "0.0"+typ
        # Storage values remain inaccessible until their initialization flags are
        # set. No unsafe/uninitialized Rust memory is needed.
        return f"let mut {rust}: Vec<{typ}> = vec![{zero}; {size}];\nlet mut rf_init_{rust} = vec![false; {size}];"

    def mark_array_initialized(self,name):
        return f"rf_init_{self.rust_name(name)}.fill(true);"

    def array_value(self,name,state,line):
        rust = self.rust_name(name)
        if name in state:
            return Value(rust,self.symbols[name],self.arrays[name])
        self.require(name in self.array_writes,line,f"read of `{name}` before definite assignment")
        return Value(f'rf_initialized(&{rust}, &rf_init_{rust}, {line}, "{name}")',self.symbols[name],self.arrays[name])

    def array_binary(self,operator,left,right,line):
        array = left if left.extent is not None else right
        if left.extent is not None and right.extent is not None:
            self.require(left.extent >= 0 and left.extent == right.extent,line,
                         "array operands must currently have equal, statically known extents")
            scalar = self.scalar_binary(operator,Value("rf_left",left.typ),Value("rf_right",right.typ),line)
            sequence = f"({iterator(left)}).zip({iterator(right)}).map(|(rf_left, rf_right)| {scalar.code})"
        elif left.extent is not None:
            scalar = self.scalar_binary(operator,Value("rf_item",left.typ),right,line)
            sequence = f"({iterator(left)}).map(|rf_item| {scalar.code})"
        else:
            scalar = self.scalar_binary(operator,left,Value("rf_item",right.typ),line)
            sequence = f"({iterator(right)}).map(|rf_item| {scalar.code})"
        return Value(f"({sequence}).collect::<Vec<{scalar.typ}>>()",scalar.typ,array.extent,sequence,True)

    def array_cast(self,value,typ,line):
        if typ == value.typ:
            return value
        scalar = self.cast(Value("rf_item",value.typ),typ,line)
        sequence = f"({iterator(value)}).map(|rf_item| {scalar.code})"
        return Value(f"({sequence}).collect::<Vec<{typ}>>()",typ,value.extent,sequence,True)

    def array_element(self,name,args,state,line,writing=False):
        self.require(len(args)==1,line,"only rank-one indexing is currently supported")
        index = self.expr(args[0],state,line)
        self.require(index.typ=="i32" and index.extent is None,line,"array subscript must be a scalar default INTEGER")
        rust = self.rust_name(name)
        self.needs_array_runtime = True
        known = integer_constant(args[0],self.plan.constants)
        if not writing and name not in state and (name,known) not in state:
            self.require(name in self.array_writes,line,f"read of `{name}` before definite assignment")
            return Value(f'rf_element(&{rust}, &rf_init_{rust}, {index.code}, {line}, "{name}")',self.symbols[name])
        return Value(f"{rust}[rf_index({index.code}, {rust}.len(), {line})]",self.symbols[name])

    def array_section(self,name,args,state,line):
        self.require(name in self.arrays,line,f"unknown array `{name}`")
        self.require(len(args)==3,line,"invalid section metadata")
        default = lambda value: isinstance(value,ast.Constant) and value.value is None
        if not default(args[2]):
            self.require(integer_constant(args[2],self.plan.constants)==1,line,"only unit-stride sections are currently supported")
        lower = ast.Constant(1) if default(args[0]) else args[0]
        upper = ast.Constant(self.arrays[name]) if default(args[1]) else args[1]
        lo,hi = self.expr(lower,state,line),self.expr(upper,state,line)
        self.require(lo.typ==hi.typ=="i32" and lo.extent is None and hi.extent is None,line,"section bounds must be scalar default INTEGER")
        start,end = integer_constant(lower,self.plan.constants),integer_constant(upper,self.plan.constants)
        extent = max(0,end-start+1) if start is not None and end is not None else -1
        self.needs_array_runtime = True
        rust = self.rust_name(name)
        if name in state:
            code = f"rf_section(&{rust}, {lo.code}, {hi.code}, {line})"
        else:
            self.require(name in self.array_writes or extent==0,line,f"read of `{name}` before definite assignment")
            code = f'rf_initialized_section(&{rust}, &rf_init_{rust}, {lo.code}, {hi.code}, {line}, "{name}")'
        return Value(code,self.symbols[name],extent)

    def reduction(self,name,value,line):
        self.require(value.extent is not None,line,f"{name.upper()} requires an array")
        self.require(value.typ in ("i32","f32","f64"),line,"only numeric reductions are currently supported")
        if name in ("sum","product"):
            initial = ("0" if name == "sum" else "1") + ("i32" if value.typ=="i32" else ".0"+value.typ)
            operator = ast.Add() if name == "sum" else ast.Mult()
            scalar = self.scalar_binary(operator,Value("rf_acc",value.typ),Value("rf_item",value.typ),line)
            code = f"({iterator(value)}).fold({initial}, |rf_acc, rf_item| {scalar.code})"
        else:
            method = "min" if name=="minval" else "max"
            empty = f"{value.typ}::MAX" if name=="minval" else f"(-{value.typ}::MAX)"
            code = f"({iterator(value)}).reduce(|rf_acc, rf_item| rf_acc.{method}(rf_item)).unwrap_or({empty})"
        return Value(code,value.typ)

    def assign_array(self,name,value,line):
        self.array_writes.add(name)
        typ,size = self.symbols[name],self.arrays[name]
        value = self.cast(value,typ,line)
        if value.extent is None:
            return f"{self.rust_name(name)} = vec![{value.code}; {size}]; " + self.mark_array_initialized(name)
        self.require(value.extent >= 0 and value.extent == size,line,
                     "array assignment requires an equal, statically known extent")
        code = value.code if value.owned else f"({value.code}).to_vec()"
        return f"{self.rust_name(name)} = {code}; " + self.mark_array_initialized(name)

    def random_number(self,args,state,line):
        self.require(len(args)==1,line,"RANDOM_NUMBER requires one positional argument")
        target = args[0]
        self.needs_random_runtime = True
        if isinstance(target,ast.Name):
            name = target.id.removeprefix("_rf_name_")
            typ = self.variable(name,line)
            self.require(name not in self.parameters,line,f"cannot assign PARAMETER `{name}`")
            self.require(typ in ("f32","f64"),line,"RANDOM_NUMBER requires a real destination")
            value = f"rf_rng.next({str(typ=='f64').lower()}) as {typ}"
            if name in self.arrays:
                self.array_writes.add(name)
                code = f"{self.rust_name(name)} = (0..{self.arrays[name]}).map(|_| {value}).collect(); " + self.mark_array_initialized(name)
            else:
                code = f"{self.rust_name(name)} = {value};"
            state.add(name)
            return code
        raise Unsupported(line,"RANDOM_NUMBER currently requires a whole array or scalar variable")


ARRAY_RUNTIME = '''
fn rf_index(index: i32, len: usize, line: usize) -> usize {
    assert!(index >= 1 && (index as usize) <= len,
            "Fortran line {line}: array subscript {index} is outside 1..={len}");
    index as usize - 1
}

fn rf_section<T>(values: &[T], lower: i32, upper: i32, line: usize) -> &[T] {
    if upper < lower { return &values[..0]; }
    let start = rf_index(lower, values.len(), line);
    rf_index(upper, values.len(), line);
    &values[start..upper as usize]
}

fn rf_initialized<'a, T>(values: &'a [T], initialized: &[bool], line: usize, name: &str) -> &'a [T] {
    if let Some(index) = initialized.iter().position(|&set| !set) {
        panic!("Fortran line {line}: read of uninitialized array `{name}`: element {}", index + 1);
    }
    values
}

fn rf_element<T: Copy>(values: &[T], initialized: &[bool], index: i32, line: usize, name: &str) -> T {
    let offset = rf_index(index, values.len(), line);
    assert!(initialized[offset], "Fortran line {line}: read of uninitialized array `{name}`: element {index}");
    values[offset]
}

fn rf_initialized_section<'a, T>(values: &'a [T], initialized: &[bool], lower: i32, upper: i32, line: usize, name: &str) -> &'a [T] {
    let section = rf_section(values, lower, upper, line);
    if upper >= lower {
        if let Some(offset) = initialized[(lower as usize - 1)..upper as usize].iter().position(|&set| !set) {
            let index = lower as usize + offset;
            panic!("Fortran line {line}: read of uninitialized array `{name}`: element {index}");
        }
    }
    section
}
'''

# The xorshift64* recurrence, default seed, and 24/53-bit conversion agree with
# rfort's native RANDOM_NUMBER implementation. No cryptographic claim is made.
RANDOM_RUNTIME = '''
struct RfRng(u64);
impl RfRng {
    fn next(&mut self, double: bool) -> f64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        let precision = if double { 53 } else { 24 };
        let bits = self.0.wrapping_mul(2685821657736338717) >> (64 - precision);
        bits as f64 / (1u64 << precision) as f64
    }
}
'''
