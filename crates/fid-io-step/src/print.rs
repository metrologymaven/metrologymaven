//! The deterministic printer (REQ-IO.CAD.STEP.P21-005, -006).

use crate::error::StepError;
use crate::{real, text, Ident, P21File, Param};
use fid_io_reg::Position;

/// How reals are written.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RealStyle {
    /// The real's lexeme when it carries one that parses back to the same value, the
    /// shortest round-trip form otherwise.
    #[default]
    LexemeOrShortest,
    /// Always the shortest round-trip form.
    Shortest,
}

/// Printer options.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PrintOptions {
    /// How reals are written.
    pub reals: RealStyle,
}

/// Prints `file` with default options.
pub fn print(file: &P21File) -> Result<String, StepError> {
    print_with(file, &PrintOptions::default())
}

/// Prints `file`: the envelope, the header records in order, then one `DATA;` section
/// with the instances in ascending entity number, one per line, `\n` line ends. Fails
/// with `malformed` only for content no exchange structure can hold: a non-finite real
/// (`non_finite_real`) or an instance without parts (`empty_instance`).
pub fn print_with(file: &P21File, opts: &PrintOptions) -> Result<String, StepError> {
    let mut out = String::new();
    out.push_str("ISO-10303-21;\nHEADER;\n");
    for inst in &file.header {
        if inst.parts.is_empty() {
            return Err(bad(inst.number, "empty_instance"));
        }
        for (name, params) in &inst.parts {
            write_record(&mut out, name, params, opts, inst.number)?;
            out.push_str(";\n");
        }
    }
    out.push_str("ENDSEC;\nDATA;\n");
    for (&n, inst) in &file.data {
        out.push('#');
        out.push_str(&n.to_string());
        out.push('=');
        match inst.parts.as_slice() {
            [] => return Err(bad(n, "empty_instance")),
            [(name, params)] => write_record(&mut out, name, params, opts, n)?,
            parts => {
                out.push('(');
                for (name, params) in parts {
                    write_record(&mut out, name, params, opts, n)?;
                }
                out.push(')');
            }
        }
        out.push_str(";\n");
    }
    out.push_str("ENDSEC;\nEND-ISO-10303-21;\n");
    Ok(out)
}

fn bad(entity: u64, reason: &'static str) -> StepError {
    StepError::Malformed { at: Position::byte(0), reason, entity: Some(entity) }
}

enum Work<'p> {
    P(&'p Param),
    S(&'static str),
}

fn push_items<'p>(stack: &mut Vec<Work<'p>>, items: &'p [Param]) {
    for (i, p) in items.iter().enumerate().rev() {
        stack.push(Work::P(p));
        if i > 0 {
            stack.push(Work::S(","));
        }
    }
}

/// `NAME(params)`, iteratively.
fn write_record(
    out: &mut String,
    name: &Ident,
    params: &[Param],
    opts: &PrintOptions,
    entity: u64,
) -> Result<(), StepError> {
    out.push_str(name.as_str());
    out.push('(');
    let mut stack: Vec<Work<'_>> = vec![Work::S(")")];
    push_items(&mut stack, params);
    while let Some(w) = stack.pop() {
        let p = match w {
            Work::S(s) => {
                out.push_str(s);
                continue;
            }
            Work::P(p) => p,
        };
        match p {
            Param::Int(i) => out.push_str(&i.to_string()),
            Param::Real(v, lexeme) => {
                if !v.is_finite() {
                    return Err(bad(entity, "non_finite_real"));
                }
                let authored = match (opts.reals, lexeme) {
                    (RealStyle::LexemeOrShortest, Some(l)) => {
                        let same = l.as_str().parse::<f64>().map(|x| x.to_bits() == v.to_bits()).unwrap_or(false);
                        same.then_some(l.as_str())
                    }
                    _ => None,
                };
                match authored {
                    Some(s) => out.push_str(s),
                    None => out.push_str(&real::shortest(*v)),
                }
            }
            Param::Str(t) => text::encode(out, t.raw()),
            Param::Enum(e) => {
                out.push('.');
                out.push_str(e.as_str());
                out.push('.');
            }
            Param::Bin(b) => {
                out.push('"');
                for &d in std::iter::once(&b.unused_bits()).chain(b.hex_digits()) {
                    out.push(char::from_digit(u32::from(d & 0xf), 16).map_or('0', |c| c.to_ascii_uppercase()));
                }
                out.push('"');
            }
            Param::Ref(n) => {
                out.push('#');
                out.push_str(&n.to_string());
            }
            Param::Typed(name, inner) => {
                out.push_str(name.as_str());
                out.push('(');
                stack.push(Work::S(")"));
                stack.push(Work::P(inner));
            }
            Param::List(items) => {
                out.push('(');
                stack.push(Work::S(")"));
                push_items(&mut stack, items);
            }
            Param::Unset => out.push('$'),
            Param::Derived => out.push('*'),
        }
    }
    Ok(())
}
