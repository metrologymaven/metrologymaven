#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! KRN.OUT.POL AC-4 (REQ-KRN.OUT.POL-006, REQ-KRN.OUT-004): only the `fid-filter` policy
//! entry point may turn a point set into a reduced, reweighted or reordered one.
//!
//! The scan fails closed. It reads the sources of `fid-fit` and `fid-filter` and pins every
//! public item whose parameters (generics and `where` clause included) or `self` involve one
//! of the point-set types `PointsView`, `Centred` or `ReducedPoints`, *whatever it returns*:
//! the return type is never used to filter, so a new accessor, a `-> Vec<u32>` ordering, a
//! tuple of buffers or a `Result<Self, _>` subset all fail this test until they are reviewed
//! and listed in [`ALLOWED`] with a reason.
//!
//! What counts as public:
//!
//! - `pub fn`, with any qualifiers (`const`, `async`, `unsafe`, `extern ".."`); `pub(crate)`,
//!   `pub(super)` and `pub(in ..)` do not count;
//! - in an inherent `impl`, the `pub fn`s; when the `impl` header names a point-set type,
//!   `self` and `Self` in the parameters count as that type;
//! - every method declared in a `pub trait` (they are public whatever their spelling);
//! - every method of a trait `impl` for a trait not declared in the scanned crates (a std
//!   trait such as `From` or `Index` would otherwise be a way around this test), and every
//!   method of any trait `impl` whose header names a point-set type. Implementations of a
//!   trait declared here, for other types, are covered by the trait's own entry: the compiler
//!   holds the impl to the declared signature.
//!
//! Aliases of the point-set types (`type X = PointsView<..>`, `PointsView as X`) are point-set
//! types too. Code the scan cannot see is refused instead: `include!`, `#[path = ..]` and any
//! `macro_rules!` not listed in [`ALLOWED_MACROS`] fail the test. Derived impls (`Clone`,
//! `Debug`, `PartialEq`) are not scanned; none of them changes a point set.

use std::path::{Path, PathBuf};

/// The point-set types: the fit input view, its normalised copy and the reduced set.
const POINT_SET_TYPES: &[&str] = &["PointsView", "Centred", "ReducedPoints"];

/// The only items allowed to return a reduced, reweighted or reordered point set.
const REDUCING: &[&str] = &["fid-filter/src/policy/mod.rs::apply"];

/// Every public item involving a point-set type, as `crate/relative/path.rs::item`, and why
/// it is allowed. Adding an item here is a review decision. Only the entry whose reason cites
/// REQ-KRN.OUT.POL-006 may reduce, reweight or reorder.
const ALLOWED: &[(&str, &str)] = &[
    // --- fid-filter: this leaf.
    ("fid-filter/src/policy/mod.rs::apply", "the policy entry point (REQ-KRN.OUT.POL-006)"),
    (
        "fid-filter/src/policy/mod.rs::PolicyMethod::exclude",
        "a method only proposes a mask; apply() alone turns it into a reduced set, under the guards and with its record",
    ),
    ("fid-filter/src/policy/mod.rs::ReducedPoints::exclusion", "accessor: the record of what apply() excluded"),
    ("fid-filter/src/policy/mod.rs::ReducedPoints::is_reduced", "accessor: whether apply() excluded anything"),
    (
        "fid-filter/src/policy/mod.rs::ReducedPoints::kept_indices",
        "accessor: the kept indices of a set apply() already reduced",
    ),
    (
        "fid-filter/src/policy/mod.rs::ReducedPoints::view",
        "accessor: borrows the set apply() already reduced; the record stays on ReducedPoints",
    ),
    // --- fid-fit: the fit contract (KRN.LSQ.TRAIT).
    ("fid-fit/src/contract/degen.rs::angular_coverage", "reads a Centred set, returns an angle"),
    ("fid-fit/src/contract/degen.rs::check", "degeneracy verdict on a Centred set"),
    (
        "fid-fit/src/contract/degen.rs::check_points",
        "normalisation (KRN.LSQ.TRAIT REQ-003) plus verdict: every point, count and order kept, exact inverse",
    ),
    ("fid-fit/src/contract/degen.rs::data_shape", "shape class of a Centred set"),
    ("fid-fit/src/contract/mod.rs::Fit::fit", "the fit trait: consumes every point, returns a FitResult"),
    ("fid-fit/src/contract/normalise.rs::Centred::is_empty", "accessor: no points"),
    ("fid-fit/src/contract/normalise.rs::Centred::len", "accessor: point count"),
    ("fid-fit/src/contract/normalise.rs::Centred::map", "accessor: the normalisation map"),
    (
        "fid-fit/src/contract/normalise.rs::Centred::new",
        "normalisation (KRN.LSQ.TRAIT REQ-003): every point, count and order kept, exact inverse",
    ),
    ("fid-fit/src/contract/normalise.rs::Centred::xyz", "accessor: every normalised point, in input order"),
    ("fid-fit/src/contract/params.rs::FitParams::validate", "checks parameters against the view, returns unit"),
    ("fid-fit/src/contract/points.rs::PointsView::is_empty", "accessor: no points"),
    ("fid-fit/src/contract/points.rs::PointsView::len", "accessor: point count"),
    ("fid-fit/src/contract/points.rs::PointsView::normal", "accessor: one normal by index"),
    ("fid-fit/src/contract/points.rs::PointsView::normals", "accessor: the caller's own normals slice, unchanged"),
    ("fid-fit/src/contract/points.rs::PointsView::point", "accessor: one point by index"),
    ("fid-fit/src/contract/points.rs::PointsView::xyz", "accessor: the caller's own coordinate slice, unchanged"),
    // --- fid-fit: fits and their start values; each consumes every point and returns a fit.
    ("fid-fit/src/init.rs::cone_start", "cone start values"),
    ("fid-fit/src/init.rs::cylinder_start", "cylinder start values"),
    ("fid-fit/src/init.rs::torus_start", "torus start values"),
    ("fid-fit/src/ls_cone.rs::fit_cone", "LS cone fit"),
    ("fid-fit/src/ls_cone.rs::fit_cone_from", "LS cone fit from given start values"),
    ("fid-fit/src/ls_cyl.rs::fit_cylinder", "LS cylinder fit"),
    ("fid-fit/src/ls_cyl.rs::fit_cylinder_from", "LS cylinder fit from given start values"),
    ("fid-fit/src/ls_rnd.rs::sphere_coverage", "sphere coverage of a Centred set, returns a fraction"),
    ("fid-fit/src/mcc_cyl/mod.rs::fit_mcc_cylinder", "MCC cylinder fit"),
    ("fid-fit/src/mcc_cyl/mod.rs::fit_mcc_cylinder_from", "MCC cylinder fit from a given axis"),
    ("fid-fit/src/tan_plane/mod.rs::fit_tan_plane", "tangent plane fit"),
    ("fid-fit/src/tan_rock.rs::MaterialSide::validate", "checks the material side against the view, returns unit"),
    ("fid-fit/src/vor/mod.rs::mz_circle2d", "MZ circle fit"),
];

/// `macro_rules!` definitions in the scanned crates, and why their expansions are safe.
const ALLOWED_MACROS: &[(&str, &str)] = &[(
    "fid-fit/src/contract/primitive.rs::slug_enum",
    "slug boilerplate for fieldless enums; takes no type parameter that could be a point-set type",
)];

// ---------------------------------------------------------------------------- lexer

/// Tokens of Rust source with comments dropped, every string, char or byte literal replaced
/// by `""`, lifetimes by `'`, and `->` and `::` kept as one token each.
fn tokens(src: &str) -> Vec<String> {
    let c: Vec<char> = src.chars().collect();
    let at = |i: usize| c.get(i).copied().unwrap_or('\0');
    let ident = |ch: char| ch.is_alphanumeric() || ch == '_';
    let mut out = Vec::new();
    let mut i = 0;
    while i < c.len() {
        let ch = c[i];
        if ch.is_whitespace() {
            i += 1;
        } else if ch == '/' && at(i + 1) == '/' {
            while i < c.len() && c[i] != '\n' {
                i += 1;
            }
        } else if ch == '/' && at(i + 1) == '*' {
            let mut depth = 0usize;
            while i < c.len() {
                if c[i] == '/' && at(i + 1) == '*' {
                    depth += 1;
                    i += 2;
                } else if c[i] == '*' && at(i + 1) == '/' {
                    depth -= 1;
                    i += 2;
                    if depth == 0 {
                        break;
                    }
                } else {
                    i += 1;
                }
            }
        } else if let Some(end) = raw_string_end(&c, i) {
            out.push("\"\"".to_string());
            i = end;
        } else if ch == '"' || (ch == 'b' && at(i + 1) == '"') {
            i += if ch == 'b' { 2 } else { 1 };
            while i < c.len() && c[i] != '"' {
                i += if c[i] == '\\' { 2 } else { 1 };
            }
            i += 1;
            out.push("\"\"".to_string());
        } else if ch == '\'' || (ch == 'b' && at(i + 1) == '\'') {
            let q = if ch == 'b' { i + 1 } else { i };
            if at(q + 1) == '\\' {
                // An escape: skip the backslash and the escaped character, then to the quote.
                i = q + 3;
                while i < c.len() && c[i] != '\'' {
                    i += 1;
                }
                i += 1;
                out.push("\"\"".to_string());
            } else if at(q + 2) == '\'' {
                i = q + 3;
                out.push("\"\"".to_string());
            } else {
                i = q + 1;
                while i < c.len() && ident(c[i]) {
                    i += 1;
                }
                out.push("'".to_string());
            }
        } else if ident(ch) {
            let s = i;
            while i < c.len() && ident(c[i]) {
                i += 1;
            }
            out.push(c[s..i].iter().collect());
        } else if ch == '-' && at(i + 1) == '>' {
            out.push("->".to_string());
            i += 2;
        } else if ch == ':' && at(i + 1) == ':' {
            out.push("::".to_string());
            i += 2;
        } else {
            out.push(ch.to_string());
            i += 1;
        }
    }
    out
}

/// End (exclusive) of a raw string `r#".."#` or `br".."` starting at `i`, if one does.
fn raw_string_end(c: &[char], i: usize) -> Option<usize> {
    if i > 0 && (c[i - 1].is_alphanumeric() || c[i - 1] == '_') {
        return None;
    }
    let mut j = i;
    if c.get(j) == Some(&'b') {
        j += 1;
    }
    if c.get(j) != Some(&'r') {
        return None;
    }
    j += 1;
    let mut hashes = 0;
    while c.get(j) == Some(&'#') {
        hashes += 1;
        j += 1;
    }
    if c.get(j) != Some(&'"') {
        return None;
    }
    j += 1;
    while j < c.len() {
        if c[j] == '"' && (1..=hashes).all(|h| c.get(j + h) == Some(&'#')) {
            return Some(j + 1 + hashes);
        }
        j += 1;
    }
    Some(c.len())
}

// ---------------------------------------------------------------------------- parser

fn is_ident(t: &str) -> bool {
    t.chars().next().is_some_and(|ch| ch.is_alphabetic() || ch == '_')
}

/// Index of the token closing the bracket opened at `open`.
fn closing(t: &[String], open: usize) -> usize {
    let (o, c) = match t[open].as_str() {
        "(" => ("(", ")"),
        "[" => ("[", "]"),
        "{" => ("{", "}"),
        _ => ("<", ">"),
    };
    let mut depth = 0usize;
    for (k, tok) in t.iter().enumerate().skip(open) {
        if tok == o {
            depth += 1;
        } else if tok == c {
            depth -= 1;
            if depth == 0 {
                return k;
            }
        }
    }
    t.len()
}

/// First `{` or `;` from `from` outside `()` and `[]`: the end of an item header.
fn header_end(t: &[String], from: usize) -> usize {
    let mut depth = 0isize;
    for (k, tok) in t.iter().enumerate().skip(from) {
        match tok.as_str() {
            "(" | "[" => depth += 1,
            ")" | "]" => depth -= 1,
            "{" | ";" if depth <= 0 => return k,
            _ => {}
        }
    }
    t.len()
}

/// Visibility of the item whose keyword is at `k`: skips qualifiers back to `pub`.
/// `pub(..)` is restricted and does not count.
fn is_pub(t: &[String], k: usize) -> bool {
    let mut j = k;
    while j > 0 {
        j -= 1;
        match t[j].as_str() {
            "const" | "async" | "unsafe" | "extern" | "default" | "auto" | "\"\"" => continue,
            "pub" => return true,
            _ => return false,
        }
    }
    false
}

/// Last identifier of the type path in `t` (`fid_fit::contract::PointsView<'a>` gives
/// `PointsView`); a macro variable `$name` stays `$name`.
fn path_name(t: &[String]) -> String {
    let mut k = 0;
    while k < t.len() && matches!(t[k].as_str(), "&" | "'" | "mut" | "dyn" | "!") {
        k += 1;
    }
    let mut name = String::from("?");
    while k < t.len() {
        if t[k] == "$" && k + 1 < t.len() {
            name = format!("${}", t[k + 1]);
            k += 2;
        } else if is_ident(&t[k]) {
            name = t[k].clone();
            k += 1;
        } else {
            break;
        }
        if t.get(k).map(String::as_str) == Some("::") {
            k += 1;
        } else {
            break;
        }
    }
    name
}

#[derive(Debug, Clone)]
enum Ctx {
    /// A trait body; `public` is `pub trait`.
    Trait { name: String, public: bool },
    /// An impl body. `label` is `Type` or `<Type as Trait>`; `target` is whether the header
    /// names a point-set type; `covered` is whether the trait's own declaration is scanned
    /// and covers this impl.
    Impl { label: String, inherent: bool, target: bool, covered: bool },
    /// A function body: nothing inside is a public item.
    Fn,
    /// Any other braces (module, struct, enum, macro, const block).
    Other,
}

/// One public function or method found, with whether it involves a point-set type.
#[derive(Debug)]
struct Found {
    label: String,
    involves: bool,
}

#[derive(Debug, Default)]
struct Scan {
    fns: Vec<Found>,
    macros: Vec<String>,
    blind: Vec<String>,
}

fn names_target(t: &[String], targets: &[String]) -> bool {
    t.iter().any(|x| targets.contains(x))
}

/// Names declared as `trait X` in `t`.
fn trait_names(t: &[String]) -> Vec<String> {
    t.windows(2).filter(|w| w[0] == "trait" && is_ident(&w[1])).map(|w| w[1].clone()).collect()
}

/// New aliases of `targets` declared in `t`: `type X = ..T..;` and `T as X`.
fn aliases(t: &[String], targets: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    for (k, tok) in t.iter().enumerate() {
        if tok == "type" && t.get(k + 1).is_some_and(|n| is_ident(n)) {
            let end = header_end(t, k);
            if names_target(&t[(k + 2).min(end)..end], targets) {
                out.push(t[k + 1].clone());
            }
        }
        if targets.contains(tok) && t.get(k + 1).map(String::as_str) == Some("as") {
            if let Some(a) = t.get(k + 2).filter(|a| is_ident(a) && *a != "_") {
                out.push(a.clone());
            }
        }
    }
    out.retain(|a| !targets.contains(a));
    out
}

fn scan_tokens(t: &[String], targets: &[String], local_traits: &[String]) -> Scan {
    let mut scan = Scan::default();
    let mut stack: Vec<Ctx> = Vec::new();
    let mut pending: Option<Ctx> = None;
    let mut k = 0;
    while k < t.len() {
        let next = t.get(k + 1).map(String::as_str);
        match t[k].as_str() {
            "{" => {
                stack.push(pending.take().unwrap_or(Ctx::Other));
                k += 1;
            }
            "}" => {
                stack.pop();
                k += 1;
            }
            ";" => {
                pending = None;
                k += 1;
            }
            "!" if k > 0 && (t[k - 1] == "include" || t[k - 1] == "include_str") => {
                scan.blind.push(format!("{}!", t[k - 1]));
                k += 1;
            }
            "!" if k > 0 && t[k - 1] == "macro_rules" => {
                scan.macros.push(next.unwrap_or_default().to_string());
                k += 1;
            }
            "#" if next == Some("[") && t.get(k + 2).map(String::as_str) == Some("path") => {
                scan.blind.push("#[path]".to_string());
                k += 1;
            }
            "trait" if next.is_some_and(is_ident) => {
                pending = Some(Ctx::Trait { name: t[k + 1].clone(), public: is_pub(t, k) });
                k += 2;
            }
            "impl" if pending.is_none() => {
                let end = header_end(t, k);
                let mut h = k + 1;
                if next == Some("<") {
                    h = closing(t, h) + 1;
                }
                let header = &t[h.min(end)..end];
                let head = &header[..header.iter().position(|x| x == "where").unwrap_or(header.len())];
                let mut depth = 0isize;
                let mut split = None;
                for (j, x) in head.iter().enumerate() {
                    match x.as_str() {
                        "<" => depth += 1,
                        ">" => depth -= 1,
                        "for" if depth == 0 => {
                            split = Some(j);
                            break;
                        }
                        _ => {}
                    }
                }
                let target = names_target(&t[k..end], targets);
                pending = Some(match split {
                    None => Ctx::Impl { label: path_name(head), inherent: true, target, covered: false },
                    Some(j) => {
                        let tr = path_name(&head[..j]);
                        let ty = path_name(&head[j + 1..]);
                        let covered = local_traits.contains(&tr) && !target;
                        Ctx::Impl { label: format!("<{ty} as {tr}>"), inherent: false, target, covered }
                    }
                });
                k = end;
            }
            "fn" if next.is_some_and(is_ident) => {
                let name = t[k + 1].clone();
                let end = header_end(t, k);
                let sig = &t[k..end];
                // Parameters: up to the closing `)`, plus the `where` clause; never the return type.
                let open = sig.iter().position(|x| x == "(").unwrap_or(sig.len());
                let close = if open < sig.len() { closing(sig, open) } else { sig.len() };
                let mut params: Vec<String> = sig[..(close + 1).min(sig.len())].to_vec();
                if let Some(w) = sig.iter().skip(close).position(|x| x == "where") {
                    params.extend_from_slice(&sig[close + w..]);
                }
                let uses_self = params.iter().any(|x| x == "self" || x == "Self");
                let in_fn = stack.iter().any(|c| matches!(c, Ctx::Fn));
                let (public, label, self_target) = match stack.last() {
                    Some(Ctx::Trait { name: tr, public }) => (*public, format!("{tr}::{name}"), false),
                    Some(Ctx::Impl { label, inherent: true, target, .. }) => {
                        (is_pub(t, k), format!("{label}::{name}"), *target)
                    }
                    Some(Ctx::Impl { label, inherent: false, target, covered }) => {
                        (!covered, format!("{label}::{name}"), *target)
                    }
                    Some(Ctx::Fn | Ctx::Other) | None => (is_pub(t, k), name.clone(), false),
                };
                if public && !in_fn {
                    let involves = names_target(&params, targets) || (self_target && uses_self);
                    scan.fns.push(Found { label, involves });
                }
                if t.get(end).map(String::as_str) == Some("{") {
                    pending = Some(Ctx::Fn);
                }
                k = end;
            }
            _ => k += 1,
        }
    }
    scan
}

/// What a scan of several files found.
struct Report {
    /// Public items involving a point-set type, as `path::item`, sorted.
    found: Vec<String>,
    /// `macro_rules!` definitions, as `path::name`, sorted.
    macros: Vec<String>,
    /// Constructs that pull in code the scan cannot read.
    blind: Vec<String>,
    /// Number of public functions and methods seen.
    seen: usize,
}

/// Scans `(relative path, source)` pairs.
fn scan_sources(files: &[(String, String)]) -> Report {
    let toks: Vec<(String, Vec<String>)> = files.iter().map(|(r, s)| (r.clone(), tokens(s))).collect();
    let mut targets: Vec<String> = POINT_SET_TYPES.iter().map(|s| s.to_string()).collect();
    loop {
        let mut new: Vec<String> = toks.iter().flat_map(|(_, t)| aliases(t, &targets)).collect();
        new.sort();
        new.dedup();
        if new.is_empty() {
            break;
        }
        targets.extend(new);
    }
    let local_traits: Vec<String> = toks.iter().flat_map(|(_, t)| trait_names(t)).collect();
    let mut r = Report { found: Vec::new(), macros: Vec::new(), blind: Vec::new(), seen: 0 };
    for (rel, t) in &toks {
        let s = scan_tokens(t, &targets, &local_traits);
        r.seen += s.fns.len();
        r.found.extend(s.fns.iter().filter(|f| f.involves).map(|f| format!("{rel}::{}", f.label)));
        r.macros.extend(s.macros.iter().map(|m| format!("{rel}::{m}")));
        r.blind.extend(s.blind.iter().map(|b| format!("{rel}: {b}")));
    }
    r.found.sort();
    r.macros.sort();
    r
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let mut entries: Vec<_> = std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().path()).collect();
    entries.sort();
    for p in entries {
        if p.is_dir() {
            rust_files(&p, out);
        } else if p.extension().is_some_and(|e| e == "rs") {
            out.push(p);
        }
    }
}

#[test]
fn ac4_every_public_item_on_a_point_set_is_reviewed() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let mut files = Vec::new();
    for krate in ["fid-fit", "fid-filter"] {
        let mut paths = Vec::new();
        rust_files(&crates.join(krate).join("src"), &mut paths);
        assert!(!paths.is_empty(), "{krate}: no sources");
        for p in paths {
            let rel = p.strip_prefix(crates).unwrap().to_string_lossy().replace('\\', "/");
            files.push((rel, std::fs::read_to_string(&p).unwrap()));
        }
    }
    let r = scan_sources(&files);
    assert!(r.seen > 100, "the scan found only {} public functions", r.seen);
    assert!(r.blind.is_empty(), "sources the scan cannot see: {:?}", r.blind);
    let want_macros: Vec<String> = ALLOWED_MACROS.iter().map(|(m, _)| m.to_string()).collect();
    assert_eq!(r.macros, want_macros, "macro_rules! definitions (review each new one)");
    let mut expected: Vec<String> = ALLOWED.iter().map(|(f, _)| f.to_string()).collect();
    expected.sort();
    assert_eq!(r.found, expected, "public items whose parameters or self involve a point-set type");
    assert!(ALLOWED.iter().chain(ALLOWED_MACROS).all(|(_, why)| !why.is_empty()));
    // Of those, only the policy entry point may return a reduced, reweighted or reordered set.
    let reducing: Vec<&str> =
        ALLOWED.iter().filter(|(_, why)| why.contains("REQ-KRN.OUT.POL-006")).map(|(f, _)| *f).collect();
    assert_eq!(reducing, REDUCING);
}

/// The scan over one in-memory file.
fn found_in(src: &str) -> Vec<String> {
    scan_sources(&[("x.rs".to_string(), src.to_string())]).found
}

/// Each shape that got past the first, return-type-filtered scan is caught now.
#[test]
fn scanner_catches_every_bypass_shape() {
    let cases: &[(&str, &[&str])] = &[
        // The shapes the review found.
        (
            "impl<'a> PointsView<'a> {\n    pub fn subset(&self, keep: &[u32]) -> Result<Self, FitError> { todo!() }\n}",
            &["x.rs::PointsView::subset"],
        ),
        ("pub fn coords(p: &PointsView<'_>) -> Vec<f64> { todo!() }", &["x.rs::coords"]),
        ("pub fn order(p: &PointsView<'_>) -> Vec<u32> { todo!() }", &["x.rs::order"]),
        ("pub fn split(p: &PointsView<'_>) -> (Vec<f64>, Option<Vec<f64>>) { todo!() }", &["x.rs::split"]),
        ("pub const fn count(c: &Centred) -> usize { 0 }", &["x.rs::count"]),
        ("pub trait Reduce {\n    fn reduce(&self, p: &PointsView<'_>) -> Vec<f64>;\n}", &["x.rs::Reduce::reduce"]),
        // More of the same family.
        ("impl Centred { pub fn take(self) -> Vec<f64> { todo!() } }", &["x.rs::Centred::take"]),
        ("pub unsafe extern \"C\" fn raw(p: &ReducedPoints) -> u8 { 0 }", &["x.rs::raw"]),
        ("pub fn arr(\n    p: &PointsView<'_>,\n) -> [f64; 3] { [0.0; 3] }", &["x.rs::arr"]),
        ("pub fn g<T: AsRef<Centred>>(t: T) -> u8 { 0 }", &["x.rs::g"]),
        ("pub fn w<T>(t: T) -> u8 where T: Into<PointsView<'static>> { 0 }", &["x.rs::w"]),
        ("pub type Pv<'a> = PointsView<'a>;\npub fn al(p: Pv<'_>) -> u8 { 0 }", &["x.rs::al"]),
        ("use fid_fit::contract::PointsView as V;\npub fn ren(p: &V) -> u8 { 0 }", &["x.rs::ren"]),
        (
            "pub trait Sub { fn sub(&self) -> Vec<f64>; }\nimpl Sub for PointsView<'_> { fn sub(&self) -> Vec<f64> { todo!() } }",
            &["x.rs::<PointsView as Sub>::sub"],
        ),
        (
            "impl From<&PointsView<'_>> for Vec<f64> { fn from(p: &PointsView<'_>) -> Self { todo!() } }",
            &["x.rs::<Vec as From>::from"],
        ),
        (
            "impl core::ops::Index<u32> for Centred { type Output = f64; fn index(&self, i: u32) -> &f64 { todo!() } }",
            &["x.rs::<Centred as Index>::index"],
        ),
        ("mod inner { pub fn deep(p: PointsView) -> u8 { 0 } }", &["x.rs::deep"]),
    ];
    for (src, want) in cases {
        assert_eq!(found_in(src), want.to_vec(), "{src}");
    }
}

/// Things that are not public items, or do not take a point set, stay out of the list.
#[test]
fn scanner_ignores_what_is_not_public_or_not_a_point_set() {
    let src = r##"
// pub fn in_comment(p: &PointsView) -> Vec<f64>
/* pub fn in_block(p: &PointsView) /* nested */ -> Vec<f64> */
/// Doc: `pub fn in_doc(p: &PointsView) -> Vec<f64>`
const S: &str = "pub fn in_string(p: &PointsView) -> Vec<f64> {";
const R: &str = r#"pub fn in_raw(p: &PointsView) {"#;
const C: char = '{';
const Q: char = '\'';
pub(crate) fn krate(p: &PointsView<'_>) -> Vec<f64> { todo!() }
pub(super) fn sup(p: &PointsView<'_>) -> Vec<f64> { todo!() }
fn private(p: &PointsView<'_>) -> Vec<f64> { todo!() }
pub fn outer() -> u8 {
    pub fn nested(p: &PointsView<'_>) -> Vec<f64> { todo!() }
    0
}
pub fn builds_one(xyz: &[f64]) -> PointsView<'_> { todo!() }
pub struct Hook { pub cb: fn(&PointsView<'_>) -> u8 }
impl Normalisation { pub fn scale(&self) -> f64 { 1.0 } }
impl<'a> PointsView<'a> { fn private_helper(&self) -> u8 { 0 } pub fn new(x: &'a [f64]) -> Self { todo!() } }
pub trait Fit { fn fit(&self, p: &PointsView<'_>) -> u8; }
impl Fit for LsPlane { fn fit(&self, p: &PointsView<'_>) -> u8 { 0 } }
pub fn after<'a>(p: &'a [f64]) -> u8 { 0 }
"##;
    assert_eq!(found_in(src), ["x.rs::Fit::fit"]);
}

/// Code the scan cannot read is reported, not skipped.
#[test]
fn scanner_reports_blind_spots() {
    let r = scan_sources(&[(
        "x.rs".to_string(),
        "macro_rules! m { () => {} }\ninclude!(\"gen.rs\");\n#[path = \"other.rs\"] mod o;".to_string(),
    )]);
    assert_eq!(r.macros, ["x.rs::m"]);
    assert_eq!(r.blind, ["x.rs: include!", "x.rs: #[path]"]);
}
