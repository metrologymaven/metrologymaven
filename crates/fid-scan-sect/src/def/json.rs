//! The canonical JSON of a definition (REQ-SCAN.SECT.DEF-005, AC-5).
//!
//! Kernel canonical JSON (KRN.MAN.KCI, RFC 8785 restricted to strings, arrays and objects):
//! object keys in UTF-16 order, no whitespace, every value a string. The schemas are fixed
//! here, so every object below lists its keys already sorted and writing cannot fail.
//!
//! - an `f64` is the 16 lowercase hex digits of its IEEE 754 bits (`f64::to_bits`), so equal
//!   values and only equal values have equal text; the constructors store `-0.0` as `+0.0`,
//!   so each definition has exactly one encoding;
//! - a count is its decimal digits;
//! - the frame (required in every definition) is its 32 lowercase hex digits (`FrameId`
//!   text form);
//! - a 16-byte ID (CAD body, derived topology ID, constructed element) is the lowercase
//!   hyphenated UUID text; an imported topology label is the label text, escaped.
//!
//! Format `fid.scan.sect.def/1`, which fixes SI units (metres and radians) for every value:
//!
//! ```text
//! {"format":F,"frame":R,"plane":P,"type":"single"}
//! {"base":P,"format":F,"frame":R,"spacing":X,"span":{"count":N}|{"extent":X},"type":"parallel"}
//! {"axis_dir":V,"axis_point":V,"format":F,"frame":R,"ref_dir":V,"span":{"count":N}|{"sweep":X},"step":X,"type":"radial"}
//! {"digest":"sha256:..","element":E,"format":F,"frame":R,"type":"from_element"}
//! P = {"normal":V,"point":V}   V = [X,X,X]
//! E = {"body":U,"face":{"derived":U}|{"imported":L},"type":"cad_face"} | {"id":U,"type":"constructed"}
//! ```

use fid_geo_cad::TopologyId;

use super::resolve::ElementRef;
use super::{ParallelSpan, Plane, RadialSpan, SectionDef, SectionKind};

/// The format tag of the canonical JSON.
pub const FORMAT: &str = "fid.scan.sect.def/1";

enum V {
    S(String),
    A(Vec<V>),
    /// Keys in canonical (UTF-16) order; checked by the tests against `fid-kci`'s writer.
    O(Vec<(&'static str, V)>),
}

fn x(v: f64) -> V {
    V::S(format!("{:016x}", v.to_bits()))
}

fn v3(a: [f64; 3]) -> V {
    V::A(vec![x(a[0]), x(a[1]), x(a[2])])
}

fn s(t: &str) -> V {
    V::S(t.to_string())
}

fn uuid(b: &[u8; 16]) -> V {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut t = String::with_capacity(36);
    for (i, byte) in b.iter().enumerate() {
        if matches!(i, 4 | 6 | 8 | 10) {
            t.push('-');
        }
        t.push(char::from(HEX[usize::from(byte >> 4)]));
        t.push(char::from(HEX[usize::from(byte & 15)]));
    }
    V::S(t)
}

fn plane(p: &Plane) -> V {
    V::O(vec![("normal", v3(p.normal())), ("point", v3(p.point()))])
}

fn element(e: &ElementRef) -> V {
    match e {
        ElementRef::CadFace { body, face } => {
            let face = match face {
                TopologyId::Derived(b) => V::O(vec![("derived", uuid(b))]),
                TopologyId::Imported(l) => V::O(vec![("imported", s(l.as_str()))]),
            };
            V::O(vec![("body", uuid(body)), ("face", face), ("type", s("cad_face"))])
        }
        ElementRef::Constructed(id) => V::O(vec![("id", uuid(&id.to_bytes())), ("type", s("constructed"))]),
    }
}

fn document(def: &SectionDef) -> V {
    let frame = || V::S(def.frame().to_string());
    match def.kind() {
        SectionKind::Single(p) => {
            V::O(vec![("format", s(FORMAT)), ("frame", frame()), ("plane", plane(p)), ("type", s("single"))])
        }
        SectionKind::Parallel(p) => {
            let span = match p.span() {
                ParallelSpan::Count(k) => V::O(vec![("count", V::S(k.to_string()))]),
                ParallelSpan::Extent(e) => V::O(vec![("extent", x(e))]),
            };
            V::O(vec![
                ("base", plane(p.base())),
                ("format", s(FORMAT)),
                ("frame", frame()),
                ("spacing", x(p.spacing())),
                ("span", span),
                ("type", s("parallel")),
            ])
        }
        SectionKind::Radial(r) => {
            let span = match r.span() {
                RadialSpan::Count(k) => V::O(vec![("count", V::S(k.to_string()))]),
                RadialSpan::Sweep(w) => V::O(vec![("sweep", x(w))]),
            };
            V::O(vec![
                ("axis_dir", v3(r.axis_dir())),
                ("axis_point", v3(r.axis_point())),
                ("format", s(FORMAT)),
                ("frame", frame()),
                ("ref_dir", v3(r.ref_dir())),
                ("span", span),
                ("step", x(r.step())),
                ("type", s("radial")),
            ])
        }
        SectionKind::FromElement(f) => V::O(vec![
            ("digest", s(f.digest().as_str())),
            ("element", element(f.element())),
            ("format", s(FORMAT)),
            ("frame", frame()),
            ("type", s("from_element")),
        ]),
    }
}

/// RFC 8785 §3.2.2.2 string escaping: `"` and `\` escaped, the C0 controls as `\b`, `\t`,
/// `\n`, `\f`, `\r` or `\u00xx` (lowercase hex), everything else verbatim.
fn write_string(out: &mut String, t: &str) {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    out.push('"');
    for c in t.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{8}' => out.push_str("\\b"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\u{c}' => out.push_str("\\f"),
            '\r' => out.push_str("\\r"),
            c if (c as u32) < 0x20 => {
                let b = c as u32 as usize;
                out.push_str("\\u00");
                out.push(char::from(HEX[b >> 4]));
                out.push(char::from(HEX[b & 15]));
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

fn write(out: &mut String, v: &V) {
    match v {
        V::S(t) => write_string(out, t),
        V::A(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write(out, item);
            }
            out.push(']');
        }
        V::O(members) => {
            out.push('{');
            for (i, (k, item)) in members.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_string(out, k);
                out.push(':');
                write(out, item);
            }
            out.push('}');
        }
    }
}

/// The canonical JSON bytes of `def` (module documentation).
pub fn canonical_json(def: &SectionDef) -> Vec<u8> {
    let mut out = String::new();
    write(&mut out, &document(def));
    out.into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;
    use fid_kci::kci::json::{to_canonical, Json};

    fn kci(v: &V) -> Json {
        match v {
            V::S(t) => Json::Str(t.clone()),
            V::A(items) => Json::Arr(items.iter().map(kci).collect()),
            // Reversed, so the kci writer has to sort the keys itself.
            V::O(members) => Json::Obj(members.iter().rev().map(|(k, item)| (k.to_string(), kci(item))).collect()),
        }
    }

    #[test]
    fn fixed_key_order_and_escaping_match_the_kci_writer() {
        let tricky = "q\"b\\s/\u{1}\u{8}\t\n\u{c}\r\u{1f}\u{7f}é\u{2028}😀";
        let doc = V::O(vec![
            ("axis_dir", V::A(vec![s(tricky), x(-0.0), x(1.0)])),
            ("axis_point", V::O(vec![("derived", s("a")), ("imported", s(tricky))])),
            ("base", s("b")),
            ("body", s("c")),
            ("count", s("1")),
            ("digest", s("d")),
            ("element", s("e")),
            ("extent", s("f")),
            ("face", s("g")),
            ("format", s(FORMAT)),
            ("frame", s("r")),
            ("id", s("h")),
            ("normal", s("i")),
            ("plane", s("j")),
            ("point", s("k")),
            ("ref_dir", s("l")),
            ("spacing", s("m")),
            ("span", s("n")),
            ("step", s("o")),
            ("sweep", s("p")),
            ("type", s("q")),
        ]);
        let mut mine = String::new();
        write(&mut mine, &doc);
        assert_eq!(Ok(mine.into_bytes()), to_canonical(&kci(&doc)));
    }
}
