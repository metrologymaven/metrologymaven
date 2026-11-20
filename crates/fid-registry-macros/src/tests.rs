//! Expansion tests: AC-3 (same input, same generated code), AC-4 (no `unsafe`, no static
//! constructor, no linker section in the generated code of `demo_ok`) and the error paths of
//! the expansion functions (errors, never panics).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use crate::{command, schema};
use proc_macro2::{TokenStream, TokenTree};
use quote::{quote, ToTokens};
use syn::{Item, Meta};

const DEMO_OK: &str = include_str!("../../../fixtures/core/reg/def/macro/demo_ok/lib.rs");

/// Expands every macro use in `demo_ok` the way the compiler would, returning only the
/// generated code (the handler bodies, written by hand, are left out).
fn expand_demo() -> Vec<String> {
    let file = syn::parse_file(DEMO_OK).expect("demo_ok parses");
    let mut out = Vec::new();
    for item in file.items {
        match item {
            Item::Struct(s) if derives_fid_schema(&s.attrs) => {
                out.push(schema::expand(s.into_token_stream()).expect("derive").to_string());
            }
            Item::Enum(e) if derives_fid_schema(&e.attrs) => {
                out.push(schema::expand(e.into_token_stream()).expect("derive").to_string());
            }
            Item::Fn(mut f) => {
                let Some(pos) = f.attrs.iter().position(|a| a.path().is_ident("fid_command")) else { continue };
                let attr = f.attrs.remove(pos);
                let Meta::List(list) = attr.meta else { panic!("fid_command takes arguments") };
                let expanded = command::expand_command(list.tokens, f.to_token_stream()).expect("fid_command");
                // Drop the handler itself (it is re-emitted unchanged), keep the generated fn.
                let generated = expanded.to_string();
                let handler = f.to_token_stream().to_string();
                out.push(generated.replacen(&handler, "", 1));
            }
            Item::Macro(m) if m.mac.path.is_ident("fid_commands") => {
                out.push(command::expand_list(m.mac.tokens).expect("fid_commands").to_string());
            }
            _ => {}
        }
    }
    out
}

fn derives_fid_schema(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|a| a.path().is_ident("derive") && a.meta.to_token_stream().to_string().contains("FidSchema"))
}

fn idents(ts: TokenStream, out: &mut Vec<String>) {
    for tt in ts {
        match tt {
            TokenTree::Ident(i) => out.push(i.to_string()),
            TokenTree::Group(g) => idents(g.stream(), out),
            _ => {}
        }
    }
}

#[test]
fn ac4_generated_code_has_no_unsafe_or_linker_tricks() {
    let generated = expand_demo();
    // 7 derives, 3 commands, 1 list.
    assert_eq!(generated.len(), 11, "{generated:#?}");
    let forbidden = [
        "unsafe",
        "static",
        "link_section",
        "used",
        "no_mangle",
        "export_name",
        "ctor",
        "init_array",
        "linkme",
        "inventory",
        "distributed_slice",
        "global_asm",
        "asm",
    ];
    for code in &generated {
        let mut found = Vec::new();
        idents(code.parse().expect("generated code is tokens"), &mut found);
        for word in forbidden {
            assert!(!found.iter().any(|i| i == word), "generated code contains `{word}`:\n{code}");
        }
    }
}

#[test]
fn ac3_expansion_is_deterministic() {
    assert_eq!(expand_demo(), expand_demo());
}

#[test]
fn ac6_declarations_list_keeps_source_order() {
    let out = command::expand_list(quote!(b, a, m::c)).expect("list").to_string();
    let b = out.find("__fid_command_b").expect("b");
    let a = out.find("__fid_command_a").expect("a");
    let c = out.find("m :: __fid_command_c").expect("c");
    assert!(b < a && a < c, "{out}");
}

fn derive_err(input: TokenStream) -> String {
    match schema::expand(input) {
        Ok(t) => panic!("expected an error, got {t}"),
        Err(e) => e.to_string(),
    }
}

#[test]
fn derive_errors() {
    assert!(derive_err(quote!(
        struct P {
            d: f64,
        }
    ))
    .contains("f64 field `d` has no quantity"));
    assert!(derive_err(quote!(
        struct P {
            d: Option<Vec<f64>>,
        }
    ))
    .contains("f64 field `d` has no quantity"));
    assert!(derive_err(quote!(
        struct P {
            #[fid(unit = "mm")]
            d: u32,
        }
    ))
    .contains("has a unit but no quantity"));
    assert!(derive_err(quote!(
        struct P {
            #[fid(dimensionless_unchecked)]
            d: f64,
        }
    ))
    .contains("unknown #[fid] field attribute `dimensionless_unchecked`"));
    assert!(derive_err(quote!(
        struct P<T> {
            d: T,
        }
    ))
    .contains("generic"));
    assert!(derive_err(quote!(
        struct P(u32);
    ))
    .contains("named fields"));
    assert!(derive_err(quote!(
        enum E {
            A(u32),
        }
    ))
    .contains("unit variant"));
    assert!(derive_err(quote!(
        enum E {}
    ))
    .contains("at least one variant"));
    assert!(derive_err(quote!(
        enum E {
            A,
            #[fid(rename = "a")]
            B,
        }
    ))
    .contains("used twice"));
    assert!(derive_err(quote!(
        struct P {
            #[fid(bogus = 1)]
            d: u32,
        }
    ))
    .contains("unknown #[fid] field attribute"));
    assert!(derive_err(quote!(
        struct P {
            #[fid(default = 5u32)]
            d: u32,
        }
    ))
    .contains("no type suffix"));
    assert!(derive_err(quote!(
        struct P {
            #[fid(default = -true)]
            d: bool,
        }
    ))
    .contains("numeric default"));
    assert!(derive_err(quote!(
        struct P {
            #[fid(default = SOME_CONST)]
            d: u32,
        }
    ))
    .contains("must be a boolean, integer, float or string literal"));
    assert!(derive_err(quote!(
        struct P {
            next: Box<Self>,
        }
    ))
    .contains("recursive"));
    assert!(derive_err(quote!(
        struct P {
            next: Option<Vec<P>>,
        }
    ))
    .contains("recursive"));
    // A dimensionless quantity, a quantity on a Vec, a same-named type from another module and
    // a negative default all expand.
    assert!(schema::expand(quote!(
        struct P {
            #[fid(quantity = "dimensionless", default = -1)]
            r: f64,
            other: other::P,
            others: Vec<crate::other::P>,
        }
    ))
    .is_ok());
    let out = schema::expand(quote!(
        struct P {
            #[fid(quantity = "length", default = -2.5)]
            d: f64,
            n: u32,
        }
    ))
    .unwrap()
    .to_string();
    assert!(out.contains("number_schema_of :: < f64 >"), "{out}");
    assert!(out.contains("schema_of :: < u32 >") && !out.contains("number_schema_of :: < u32 >"), "{out}");
    assert!(out.contains("AcceptsDefault < :: fid_registry :: decl :: rt :: FloatLit >"), "{out}");
    assert!(schema::expand(quote!(
        struct P {
            #[fid(quantity = "length", unit = "mm")]
            d: Vec<f64>,
        }
    ))
    .is_ok());
}

#[test]
fn path_like_fields() {
    let out = schema::expand(quote!(
        struct P {
            #[fid(path_like, extensions = ["step", "stl"])]
            source: String,
            #[fid(path_like)]
            target: Option<String>,
        }
    ))
    .unwrap()
    .to_string();
    assert!(out.contains("path_like_schema_of :: < String >"), "{out}");
    assert!(out.contains("path_like_schema_of :: < Option < String > >"), "{out}");
    assert!(out.contains("\"x-extensions\""), "{out}");
    assert!(out.contains("text (\"step\") , :: fid_registry :: decl :: rt :: text (\"stl\")"), "{out}");
    for (attr, want) in [
        (quote!(#[fid(extensions = ["stl"])]), "`extensions` needs `path_like`"),
        (quote!(#[fid(path_like, extensions = [])]), "at least one extension"),
        (quote!(#[fid(path_like, extensions = ["stl", "step"])]), "distinct and in sorted order"),
        (quote!(#[fid(path_like, extensions = ["stl", "stl"])]), "distinct and in sorted order"),
        (quote!(#[fid(path_like, extensions = ["STL"])]), "is not a valid extension"),
        (quote!(#[fid(path_like, extensions = [".stl"])]), "is not a valid extension"),
        (quote!(#[fid(path_like, extensions = ["abcdefghijklmnopq"])]), "is not a valid extension"),
        (quote!(#[fid(path_like, extensions = [STL])]), "must be a string literal"),
        (quote!(#[fid(path_like, extensions = "stl")]), "expected square brackets"),
        (quote!(#[fid(path_like = true)]), "`path_like` is a flag"),
        (quote!(#[fid(path_like, path_like)]), "given twice"),
        (quote!(#[fid(path_like, quantity = "length")]), "cannot carry a quantity"),
    ] {
        let err = derive_err(quote!(
            struct P {
                #attr
                f: String,
            }
        ));
        assert!(err.contains(want), "{attr}: {err}");
    }
}

fn command_err(args: TokenStream, item: TokenStream) -> String {
    match command::expand_command(args, item) {
        Ok(t) => panic!("expected an error, got {t}"),
        Err(e) => e.into_compile_error().to_string(),
    }
}

#[test]
fn command_errors() {
    let handler = quote!(
        fn h(c: (), p: P) -> Result<R, E> {
            todo!()
        }
    );
    let all = quote!(
        id = "demo.h",
        major = 1,
        title = "H",
        description = "H.",
        side_effect = R0,
        mcp_projection = Exposed,
        determinism = D0,
        idempotency = Natural,
        fs_effect = None,
        undo = NotUndoable,
        preconditions = [],
        effects = [],
        algorithms = [],
        exposure = [Cli],
        lifecycle(state = "active"),
        web = Available
    );
    assert!(command::expand_command(all.clone(), handler.clone()).is_ok());

    let e = command_err(quote!(id = "demo.h"), handler.clone());
    for field in ["major", "side_effect", "mcp_projection", "determinism", "web", "lifecycle", "exposure"] {
        assert!(e.contains(&format!("missing required field `{field}`")), "{field}: {e}");
    }
    let partial = quote!(
        id = "demo.h",
        major = 1,
        title = "H",
        description = "H.",
        side_effect = R0,
        mcp_projection = Exposed,
        determinism = D0,
        idempotency = Natural,
        fs_effect = None,
        undo = NotUndoable,
        preconditions = [],
        effects = [],
        algorithms = [],
        exposure = [Cli],
        lifecycle(state = "active"),
        web = Partial
    );
    assert!(command_err(partial, handler.clone()).contains("missing required field `web_note`"));

    let bad = |extra: TokenStream| {
        let mut args = all.clone();
        args.extend(quote!(,));
        args.extend(extra);
        command_err(args, handler.clone())
    };
    assert!(bad(quote!(side_effect = W2)).contains("given twice"));
    assert!(bad(quote!(colour = 3)).contains("unknown #[fid_command] field"));

    let swap = |from: &str, to: &str| {
        let args: TokenStream = all.to_string().replacen(from, to, 1).parse().expect("tokens");
        command_err(args, handler.clone())
    };
    assert!(swap("\"demo.h\"", "\"Demo.H\"").contains("id must be a command name"));
    assert!(swap("exposure = [Cli]", "exposure = [Cli, Cli]").contains("twice"));
    assert!(swap("algorithms = []", "algorithms = [\"fit.circle@1.0.0\"]").contains("algorithm ID"));
    assert!(swap("preconditions = []", "preconditions = [(expr = \"true\", bound = 0, message_key = \"a.b\")]")
        .contains("bound must be"));
    assert!(swap("preconditions = []", "preconditions = [(expr = \"true\", bound = 5, message_key = \"A\")]")
        .contains("message_key"));

    for (sig, why) in [
        (
            quote!(
                fn h(p: P) -> Result<R, E> {}
            ),
            "exactly two arguments",
        ),
        (
            quote!(
                fn h(c: (), p: P) -> R {}
            ),
            "Result<R, E>",
        ),
        (
            quote!(
                fn h(c: (), p: P) -> io::Result<R> {}
            ),
            "Result<R, E>",
        ),
        (
            quote!(
                fn h(c: (), p: P) {}
            ),
            "must return a Result",
        ),
        (
            quote!(
                fn h<T>(c: (), p: T) -> Result<R, E> {}
            ),
            "generic",
        ),
        (
            quote!(
                async fn h(c: (), p: P) -> Result<R, E> {}
            ),
            "async",
        ),
    ] {
        assert!(command_err(all.clone(), sig).contains(why), "{why}");
    }
}

#[test]
fn list_errors() {
    let e = command::expand_list(quote!(a, b, a)).map(|t| t.to_string());
    assert!(e.is_err_and(|e| e.to_string().contains("listed twice")));
    assert!(command::expand_list(quote!(a::<u8>)).is_err());
    assert!(command::expand_list(quote!()).is_ok());
}
