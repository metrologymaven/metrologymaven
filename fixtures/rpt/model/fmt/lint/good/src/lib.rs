// Fixture for tools/rpt_float_lint.py: code that must give no finding. Not a workspace
// member: never compiled.
//! Doc comment: format!("{}", 1.5_f64) and (x as f64).to_string() are not code here.

/* Block comment /* nested */ with format!("{:.3}", 2.0) */

pub struct Holder<'a> {
    pub text: &'a str,
    pub value: u32,
}

pub fn ok(h: &Holder<'_>, n: u32, t: ((u8, u8), u8)) -> String {
    let quote = '"';
    let tick = '\'';
    let raw = r#"format!("{}", 1.5)"#;
    let s = "{:.3} is only text outside a macro";
    let _ = (0..10).len() + (t.0).1 as usize + t.1 as usize;
    assert!(h.value > 1, "value must exceed one");
    debug_assert!(h.value < 2);
    format!("{quote}{tick}{raw}{s}{}{:>8}{:?}{:#x}{:08}", h.text, n, n, n, n)
}

pub fn exact(n: u32) -> String {
    n.to_string()
}
