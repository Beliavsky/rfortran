// SPDX-License-Identifier: MIT
//! Inline checked arithmetic in the semantic kind; storage remains I64.
use super::*;

fn require(b: &mut FunctionBuilder, ex: &Externs, okay: ir::Value, k: u8, line: i64, reason: i64) {
    let yes = b.create_block();
    let no = b.create_block();
    b.ins().brif(okay, yes, &[], no, &[]);
    b.switch_to_block(no);
    b.seal_block(no);
    let kind = b.ins().iconst(ir::types::I64, k as i64);
    let line = b.ins().iconst(ir::types::I64, line);
    let reason = b.ins().iconst(ir::types::I64, reason);
    b.ins().call(ex.integer_error, &[kind, line, reason]);
    b.ins().jump(yes, &[]);
    b.switch_to_block(yes);
    b.seal_block(yes);
}
fn narrow(
    b: &mut FunctionBuilder,
    ex: &Externs,
    v: ir::Value,
    k: u8,
    checks: bool,
    line: i64,
) -> ir::Value {
    if k == 8 {
        return v;
    }
    let ty = match k {
        1 => ir::types::I8,
        2 => ir::types::I16,
        _ => ir::types::I32,
    };
    let small = b.ins().ireduce(ty, v);
    let result = b.ins().sextend(ir::types::I64, small);
    if checks {
        let okay = b.ins().icmp(ir::condcodes::IntCC::Equal, v, result);
        require(b, ex, okay, k, line, 0);
    }
    result
}
fn arithmetic(
    b: &mut FunctionBuilder,
    ex: &Externs,
    op: &str,
    k: u8,
    a: ir::Value,
    v: ir::Value,
    checks: bool,
    line: i64,
) -> ir::Value {
    let (result, overflow) = match op {
        "add" => b.ins().sadd_overflow(a, v),
        "sub" => b.ins().ssub_overflow(a, v),
        _ => b.ins().smul_overflow(a, v),
    };
    if checks {
        let okay = b.ins().icmp_imm(ir::condcodes::IntCC::Equal, overflow, 0);
        require(b, ex, okay, k, line, 0);
    }
    narrow(b, ex, result, k, checks, line)
}
pub(super) fn emit(
    b: &mut FunctionBuilder,
    ex: &Externs,
    op: &str,
    k: u8,
    a: CgVal,
    v: ir::Value,
    checks: bool,
    line: i64,
) -> ir::Value {
    use ir::{condcodes::IntCC, types};
    let a = match a {
        CgVal::F64(value) => {
            let value = b.ins().trunc(value);
            if checks {
                let low = b.ins().f64const(-2.0f64.powi(k as i32 * 8 - 1));
                let high = b.ins().f64const(2.0f64.powi(k as i32 * 8 - 1));
                let lo = b
                    .ins()
                    .fcmp(ir::condcodes::FloatCC::GreaterThanOrEqual, value, low);
                let hi = b.ins().fcmp(ir::condcodes::FloatCC::LessThan, value, high);
                let okay = b.ins().band(lo, hi);
                require(b, ex, okay, k, line, 0);
            }
            b.ins().fcvt_to_sint_sat(types::I64, value)
        }
        _ => to_i64(b, a),
    };
    match op {
        "cast" | "real" => narrow(b, ex, a, k, checks, line),
        "add" | "sub" | "mul" => arithmetic(b, ex, op, k, a, v, checks, line),
        "neg" | "abs" => {
            let zero = b.ins().iconst(types::I64, 0);
            if op == "neg" {
                return arithmetic(b, ex, "sub", k, zero, a, checks, line);
            }
            let min = b.ins().iconst(
                types::I64,
                if k == 8 {
                    i64::MIN
                } else {
                    -(1i64 << (k * 8 - 1))
                },
            );
            if checks {
                let okay = b.ins().icmp(IntCC::NotEqual, a, min);
                require(b, ex, okay, k, line, 0);
            }
            let neg = b.ins().ineg(a);
            let less = b.ins().icmp_imm(IntCC::SignedLessThan, a, 0);
            let result = b.ins().select(less, neg, a);
            narrow(b, ex, result, k, false, line)
        }
        "div" => {
            let nonzero = b.ins().icmp_imm(IntCC::NotEqual, v, 0);
            require(b, ex, nonzero, k, line, 1);
            let min = b.ins().icmp_imm(IntCC::Equal, a, i64::MIN);
            let minus = b.ins().icmp_imm(IntCC::Equal, v, -1);
            let overflow = b.ins().band(min, minus);
            if checks {
                let okay = b.ins().icmp_imm(IntCC::Equal, overflow, 0);
                require(b, ex, okay, k, line, 0);
            }
            let one = b.ins().iconst(types::I64, 1);
            let divisor = b.ins().select(overflow, one, v);
            let result = b.ins().sdiv(a, divisor);
            narrow(b, ex, result, k, checks, line)
        }
        "pow" => {
            let negative = b.create_block();
            let head = b.create_block();
            let body = b.create_block();
            let multiply = b.create_block();
            let after_multiply = b.create_block();
            let square = b.create_block();
            let done = b.create_block();
            let factor = b.append_block_param(head, types::I64);
            let exponent = b.append_block_param(head, types::I64);
            let result = b.append_block_param(head, types::I64);
            let product = b.append_block_param(after_multiply, types::I64);
            let answer = b.append_block_param(done, types::I64);
            let one = b.ins().iconst(types::I64, 1);
            let below = b.ins().icmp_imm(IntCC::SignedLessThan, v, 0);
            b.ins().brif(
                below,
                negative,
                &[],
                head,
                &[a.into(), v.into(), one.into()],
            );
            b.switch_to_block(negative);
            let okay = b.ins().icmp_imm(IntCC::NotEqual, a, 0);
            require(b, ex, okay, k, line, 1);
            let reciprocal = b.ins().sdiv(one, a);
            let magnitude = b.ins().ineg(v);
            b.ins()
                .jump(head, &[reciprocal.into(), magnitude.into(), one.into()]);
            b.switch_to_block(head);
            let finished = b.ins().icmp_imm(IntCC::Equal, exponent, 0);
            b.ins().brif(finished, done, &[result.into()], body, &[]);
            b.switch_to_block(body);
            let bit = b.ins().band_imm(exponent, 1);
            let odd = b.ins().icmp_imm(IntCC::NotEqual, bit, 0);
            b.ins()
                .brif(odd, multiply, &[], after_multiply, &[result.into()]);
            b.switch_to_block(multiply);
            let p = arithmetic(b, ex, "mul", k, result, factor, checks, line);
            b.ins().jump(after_multiply, &[p.into()]);
            b.switch_to_block(after_multiply);
            let next = b.ins().ushr_imm(exponent, 1);
            let finished = b.ins().icmp_imm(IntCC::Equal, next, 0);
            b.ins().brif(finished, done, &[product.into()], square, &[]);
            b.switch_to_block(square);
            let squared = arithmetic(b, ex, "mul", k, factor, factor, checks, line);
            b.ins()
                .jump(head, &[squared.into(), next.into(), product.into()]);
            for block in [negative, head, body, multiply, after_multiply, square, done] {
                b.seal_block(block);
            }
            b.switch_to_block(done);
            answer
        }
        _ => unreachable!(),
    }
}
