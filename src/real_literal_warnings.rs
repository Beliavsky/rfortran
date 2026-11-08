// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! Literal spelling advice, independent of the destination's numeric kind.
use super::*;

// Canonical decimal digits times ten to a signed power. Keeping the decimal
// text is essential: parsing it as f64 first would falsely declare literals
// such as 9007199254740993.0_8 exactly representable.
fn decimal(text: &str) -> Option<(String,i64)> {
    let text=text.split('_').next()?.to_ascii_lowercase().replace('d',"e");
    let (mantissa,exponent)=text.split_once('e').unwrap_or((&text,"0"));
    let fraction=mantissa.split_once('.').map_or(0,|(_,s)|s.len());
    let digits=mantissa.trim_start_matches(['+','-']).replace('.',"");
    if !digits.bytes().all(|b|b.is_ascii_digit()) {return None;}
    let digits=digits.trim_start_matches('0');
    if digits.is_empty() {return Some(("0".into(),0));}
    let trimmed=digits.trim_end_matches('0');
    let power=exponent.parse::<i64>().ok()?.checked_sub(i64::try_from(fraction).ok()?)?
        .checked_add(i64::try_from(digits.len()-trimmed.len()).ok()?)?;
    Some((trimmed.into(),power))
}

fn multiply(digits: &mut Vec<u8>, by: u8) {
    let mut carry=0;
    for digit in digits.iter_mut().rev() {
        let n=*digit*by+carry;
        *digit=n%10;
        carry=n/10;
    }
    if carry!=0 {digits.insert(0,carry);}
}

fn exact(text: &str, bytes: u8, written: &(String,i64)) -> bool {
    let number=text.split('_').next().unwrap_or(text).replace(['d','D'],"e");
    let number=if bytes==4 {number.parse::<f32>().ok().map(f64::from)}
        else {number.parse::<f64>().ok()};
    let Some(number)=number.filter(|n|n.is_finite() && *n!=0.0) else {
        return written.0=="0";
    };
    let bits=number.abs().to_bits();
    let exponent=((bits>>52)&0x7ff) as i32;
    let mut mantissa=bits&((1u64<<52)-1);
    let mut power=if exponent==0 {-1074} else {
        mantissa|=1u64<<52;
        exponent-1023-52
    };
    while mantissa%2==0 {mantissa/=2;power+=1;}
    let mut digits=mantissa.to_string().bytes().map(|b|b-b'0').collect::<Vec<_>>();
    for _ in 0..power.unsigned_abs() {multiply(&mut digits,if power<0 {5}else{2});}
    let mut decimal_power=i64::from(power.min(0));
    while digits.last()==Some(&0) {digits.pop();decimal_power+=1;}
    let digits=digits.into_iter().map(|d|char::from(d+b'0')).collect::<String>();
    written.0==digits && written.1==decimal_power
}

pub(super) fn check<'a>(a: &mut Analysis<'a>, body: &'a [Stmt], scope: usize, style: bool, excess: bool) {
    fn walk<'a>(a: &mut Analysis<'a>,body: &'a [Stmt],scope: usize,mut offset: usize,
        seen: &mut HashSet<usize>,style: bool,excess: bool) {
        let scope=a.bodies.get(&(body.as_ptr() as usize)).copied().unwrap_or(scope);
        for s in body {
            // DATA/SAVE normalization may remove a statement while retaining
            // its source marker. Inspect markers too, before the next one
            // replaces the offset, so original DATA literals are not lost.
            if let Stmt::SourceLocation(n)=s {offset=*n;}
            let literals=a.statement_tokens(offset).iter().filter_map(|t| {
                if let TokenKind::Float(text)=&t.kind {Some((text.clone(),t.span.clone()))}else{None}
            }).collect::<Vec<_>>();
            for (text,span) in literals {
                if !seen.insert(span.start) {continue;}
                if style && !text.contains('_') && !text.contains(['d','D']) {
                    a.warnings.push(Advice{category:"real-literal-kind",span:span.clone(),
                        message:format!("real literal `{text}` has no explicit kind (default REAL(4))"),related:None,
                        note:"use an explicit kind suffix such as _wp, with wp declared as the intended real kind; a D exponent also specifies double precision"});
                }
                if !excess {continue;}
                let Some(kind)=a.numeric_type(&Expr::RealLit(text.clone()),scope) else {continue;};
                let precision=match kind.bytes {4=>6,8=>15,_=>continue};
                let Some(written)=decimal(&text) else {continue;};
                let digits=written.0.len();
                if digits<=precision || exact(&text,kind.bytes,&written) {continue;}
                a.warnings.push(Advice{category:"excess-real-digits",span,
                    message:format!("real literal `{text}` supplies {digits} significant decimal digits; REAL({}) guarantees {precision}",kind.bytes),related:None,
                    note:"check the literal's intended kind; a suffix such as _wp selects a declared kind, but a wider destination or later conversion cannot restore precision already lost in the literal; exactly representable values are exempt"});
            }
            match s {
                Stmt::Function{body,..}|Stmt::Subroutine{body,..}|Stmt::Module{body,..}
                    |Stmt::DerivedType{components:body,..}=>walk(a,body,scope,offset,seen,style,excess),
                _=>for body in children(s) {walk(a,body,scope,offset,seen,style,excess);},
            }
        }
    }
    walk(a,body,scope,0,&mut HashSet::new(),style,excess);
}
