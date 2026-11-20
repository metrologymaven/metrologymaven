//! Shared helpers: error collection, name rules and type inspection.

use proc_macro2::Span;
use syn::visit::Visit;
use syn::{GenericArgument, PathArguments, Type};

/// Collects several errors so one compile reports all of them.
#[derive(Default)]
pub struct Errors(Option<syn::Error>);

impl Errors {
    pub fn push(&mut self, e: syn::Error) {
        match &mut self.0 {
            Some(all) => all.combine(e),
            None => self.0 = Some(e),
        }
    }

    pub fn at(&mut self, span: Span, msg: impl std::fmt::Display) {
        self.push(syn::Error::new(span, msg));
    }

    pub fn finish(self) -> syn::Result<()> {
        match self.0 {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }
}

/// `fid_types::CommandName::is_valid`: `[a-z0-9_.]`, starting and ending with a lowercase
/// letter or digit. Used for command IDs and precondition message keys (CORE.REG.DEF.FIELD).
pub fn is_command_name(s: &str) -> bool {
    let b = s.as_bytes();
    let edge = |c: Option<&u8>| c.is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit());
    edge(b.first())
        && edge(b.last())
        && b.iter().all(|&c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_' || c == b'.')
}

/// `fid_kci::AlgorithmId::parse`: `module.feature.method@a.b.c`, each name segment
/// `[a-z][a-z0-9_]*`, each version part a canonical `u32`.
pub fn parse_algorithm(s: &str) -> Option<([String; 3], [u32; 3])> {
    let (name, version) = s.split_once('@')?;
    let segs: Vec<&str> = name.split('.').collect();
    let vers: Vec<&str> = version.split('.').collect();
    if segs.len() != 3 || vers.len() != 3 {
        return None;
    }
    let seg_ok = |x: &str| {
        let mut b = x.bytes();
        matches!(b.next(), Some(c) if c.is_ascii_lowercase())
            && b.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_')
    };
    if !segs.iter().all(|x| seg_ok(x)) {
        return None;
    }
    let part = |x: &str| -> Option<u32> {
        if x.is_empty() || !x.bytes().all(|c| c.is_ascii_digit()) || (x.len() > 1 && x.starts_with('0')) {
            return None;
        }
        x.parse().ok()
    };
    let v = [part(vers[0])?, part(vers[1])?, part(vers[2])?];
    Some(([segs[0].to_string(), segs[1].to_string(), segs[2].to_string()], v))
}

/// `RequestOnly` -> `request_only`, `D0` -> `d0`.
pub fn snake_case(s: &str) -> String {
    let mut out = String::new();
    let mut prev_lower_or_digit = false;
    for c in s.chars() {
        if c.is_ascii_uppercase() {
            if prev_lower_or_digit {
                out.push('_');
            }
            out.push(c.to_ascii_lowercase());
            prev_lower_or_digit = false;
        } else {
            out.push(c);
            prev_lower_or_digit = c.is_ascii_lowercase() || c.is_ascii_digit();
        }
    }
    out
}

/// The single generic argument of `ty` if its last path segment is `name<T>`.
pub fn single_arg<'a>(ty: &'a Type, name: &str) -> Option<&'a Type> {
    let Type::Path(p) = ty else { return None };
    if p.qself.is_some() {
        return None;
    }
    let last = p.path.segments.last()?;
    if last.ident != name {
        return None;
    }
    let PathArguments::AngleBracketed(args) = &last.arguments else { return None };
    let mut types = args.args.iter().filter_map(|a| match a {
        GenericArgument::Type(t) => Some(t),
        _ => None,
    });
    let first = types.next()?;
    if types.next().is_some() || args.args.len() != 1 {
        return None;
    }
    Some(first)
}

/// Whether `ty` is `Option<T>` (by its last path segment, as serde does).
pub fn is_option(ty: &Type) -> bool {
    single_arg(ty, "Option").is_some()
}

/// Whether any path segment of `ty` is one of `names`, or `ty` mentions `Self`.
pub fn mentions(ty: &Type, names: &[&str]) -> bool {
    struct Finder<'n> {
        names: &'n [&'n str],
        found: bool,
    }
    impl<'ast> Visit<'ast> for Finder<'_> {
        fn visit_path_segment(&mut self, seg: &'ast syn::PathSegment) {
            if self.names.iter().any(|n| seg.ident == n) {
                self.found = true;
            }
            syn::visit::visit_path_segment(self, seg);
        }
    }
    let mut f = Finder { names, found: false };
    f.visit_type(ty);
    f.found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        assert!(is_command_name("registry.show_manifest"));
        assert!(!is_command_name("Registry.x"));
        assert!(!is_command_name("x."));
        assert!(!is_command_name(""));
        assert_eq!(snake_case("RequestOnly"), "request_only");
        assert_eq!(snake_case("D0"), "d0");
        assert_eq!(snake_case("Mm"), "mm");
        assert_eq!(parse_algorithm("fit.circle.ls@1.0.12").map(|a| a.1), Some([1, 0, 12]));
        assert!(parse_algorithm("fit.circle.ls@1.0.012").is_none());
        assert!(parse_algorithm("fit.circle@1.0.0").is_none());
        assert!(parse_algorithm("fit.Circle.ls@1.0.0").is_none());
    }
}
