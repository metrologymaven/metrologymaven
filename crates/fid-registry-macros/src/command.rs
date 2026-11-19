//! `#[fid_command(...)]` and `fid_commands![...]` (REQ-CORE.REG.DEF.MACRO-003 to -006).

use crate::util::{is_command_name, parse_algorithm, single_arg, Errors};
use proc_macro2::{Span, TokenStream};
use quote::{format_ident, quote, quote_spanned};
use syn::ext::IdentExt;
use syn::parse::{ParseStream, Parser};
use syn::punctuated::Punctuated;
use syn::spanned::Spanned;
use syn::{
    bracketed, parenthesized, Expr, ExprLit, FnArg, Ident, ItemFn, Lit, LitBool, LitInt, LitStr, Path, ReturnType,
    Token, Type,
};

/// Fields of `CommandDef` the attribute must give (CORE.REG.DEF.FIELD marks them required;
/// `web_note` is required too when `web` is not `Available`). In `CommandDef` field order.
const REQUIRED: [&str; 16] = [
    "id",
    "major",
    "title",
    "description",
    "side_effect",
    "mcp_projection",
    "determinism",
    "idempotency",
    "fs_effect",
    "undo",
    "preconditions",
    "effects",
    "algorithms",
    "exposure",
    "lifecycle",
    "web",
];

/// Fields with a default (CORE.REG.DEF.FIELD), plus the optional type cross-checks.
const OPTIONAL: [&str; 6] = ["human_only", "network", "web_note", "cli_bare", "params", "result"];

/// Largest declared precondition step bound (`fid_registry::def::MAX_PRECONDITION_BOUND`).
const MAX_BOUND: u32 = 1_000_000;

struct Precondition {
    expr: LitStr,
    bound: LitInt,
    message_key: LitStr,
}

#[derive(Default)]
struct Args {
    id: Option<LitStr>,
    major: Option<LitInt>,
    title: Option<Expr>,
    description: Option<Expr>,
    side_effect: Option<Ident>,
    mcp_projection: Option<Ident>,
    determinism: Option<Ident>,
    idempotency: Option<Ident>,
    fs_effect: Option<Ident>,
    undo: Option<Ident>,
    preconditions: Option<Vec<Precondition>>,
    effects: Option<Vec<Expr>>,
    algorithms: Option<Vec<LitStr>>,
    exposure: Option<Vec<Ident>>,
    lifecycle: Option<Vec<(Ident, Expr)>>,
    web: Option<Ident>,
    human_only: Option<LitBool>,
    network: Option<Ident>,
    web_note: Option<Expr>,
    cli_bare: Option<LitBool>,
    params: Option<Type>,
    result: Option<Type>,
    seen: Vec<String>,
}

fn list<T>(input: ParseStream<'_>, item: fn(ParseStream<'_>) -> syn::Result<T>) -> syn::Result<Vec<T>> {
    let content;
    bracketed!(content in input);
    let items = Punctuated::<T, Token![,]>::parse_terminated_with(&content, item)?;
    Ok(items.into_iter().collect())
}

fn precondition(input: ParseStream<'_>) -> syn::Result<Precondition> {
    let content;
    let paren = parenthesized!(content in input);
    let (mut expr, mut bound, mut key) = (None, None, None);
    let pairs = Punctuated::<syn::MetaNameValue, Token![,]>::parse_terminated(&content)?;
    for nv in pairs {
        let Some(name) = nv.path.get_ident().map(|i| i.to_string()) else {
            return Err(syn::Error::new(nv.path.span(), "expected expr, bound or message_key"));
        };
        let lit = match &nv.value {
            Expr::Lit(ExprLit { lit, .. }) => lit.clone(),
            other => return Err(syn::Error::new(other.span(), "precondition values must be literals")),
        };
        match (name.as_str(), lit) {
            ("expr", Lit::Str(s)) if expr.is_none() => expr = Some(s),
            ("bound", Lit::Int(i)) if bound.is_none() => bound = Some(i),
            ("message_key", Lit::Str(s)) if key.is_none() => key = Some(s),
            _ => {
                return Err(syn::Error::new(
                    nv.span(),
                    "a precondition is (expr = \"...\", bound = N, message_key = \"...\"), each once",
                ))
            }
        }
    }
    match (expr, bound, key) {
        (Some(expr), Some(bound), Some(message_key)) => Ok(Precondition { expr, bound, message_key }),
        _ => Err(syn::Error::new(paren.span.join(), "a precondition needs expr, bound and message_key")),
    }
}

fn parse_args(tokens: TokenStream) -> syn::Result<Args> {
    let mut a = Args::default();
    let parser = syn::meta::parser(|meta| {
        let Some(ident) = meta.path.get_ident() else { return Err(meta.error("expected a field name")) };
        let key = ident.unraw().to_string();
        if !REQUIRED.contains(&key.as_str()) && !OPTIONAL.contains(&key.as_str()) {
            return Err(meta.error(format!("unknown #[fid_command] field `{key}`")));
        }
        if a.seen.contains(&key) {
            return Err(meta.error(format!("`{key}` is given twice")));
        }
        a.seen.push(key.clone());
        if key == "lifecycle" {
            let mut entries: Vec<(Ident, Expr)> = Vec::new();
            meta.parse_nested_meta(|inner| {
                let Some(k) = inner.path.get_ident() else { return Err(inner.error("expected a lifecycle key")) };
                if entries.iter().any(|(e, _)| e == k) {
                    return Err(inner.error(format!("lifecycle `{k}` is given twice")));
                }
                entries.push((k.clone(), inner.value()?.parse()?));
                Ok(())
            })?;
            a.lifecycle = Some(entries);
            return Ok(());
        }
        let v = meta.value()?;
        match key.as_str() {
            "id" => a.id = Some(v.parse()?),
            "major" => a.major = Some(v.parse()?),
            "title" => a.title = Some(v.parse()?),
            "description" => a.description = Some(v.parse()?),
            "side_effect" => a.side_effect = Some(v.parse()?),
            "mcp_projection" => a.mcp_projection = Some(v.parse()?),
            "determinism" => a.determinism = Some(v.parse()?),
            "idempotency" => a.idempotency = Some(v.parse()?),
            "fs_effect" => a.fs_effect = Some(v.parse()?),
            "undo" => a.undo = Some(v.parse()?),
            "web" => a.web = Some(v.parse()?),
            "network" => a.network = Some(v.parse()?),
            "human_only" => a.human_only = Some(v.parse()?),
            "cli_bare" => a.cli_bare = Some(v.parse()?),
            "web_note" => a.web_note = Some(v.parse()?),
            "params" => a.params = Some(v.parse()?),
            "result" => a.result = Some(v.parse()?),
            "preconditions" => a.preconditions = Some(list(v, precondition)?),
            "effects" => a.effects = Some(list(v, |s| s.parse::<Expr>())?),
            "algorithms" => a.algorithms = Some(list(v, |s| s.parse::<LitStr>())?),
            "exposure" => a.exposure = Some(list(v, |s| s.parse::<Ident>())?),
            _ => return Err(meta.error("unreachable field")),
        }
        Ok(())
    });
    parser.parse2(tokens)?;
    Ok(a)
}

/// The handler's `(Ctx, P, R, E)` and its return type.
struct Signature<'a> {
    ctx: &'a Type,
    params: &'a Type,
    result: &'a Type,
    ret: &'a Type,
}

fn signature(f: &ItemFn) -> syn::Result<Signature<'_>> {
    let sig = &f.sig;
    let shape = "a command handler must be `fn(ctx: Ctx, params: P) -> Result<R, E>`";
    if !sig.generics.params.is_empty() || sig.generics.where_clause.is_some() {
        return Err(syn::Error::new(sig.generics.span(), format!("{shape}; it cannot be generic")));
    }
    if let Some(t) = &sig.asyncness {
        return Err(syn::Error::new(t.span, format!("{shape}; it cannot be async")));
    }
    if let Some(t) = &sig.unsafety {
        return Err(syn::Error::new(t.span, format!("{shape}; it cannot be unsafe")));
    }
    if let Some(t) = &sig.constness {
        return Err(syn::Error::new(t.span, format!("{shape}; it cannot be const")));
    }
    if let Some(abi) = &sig.abi {
        return Err(syn::Error::new(abi.span(), format!("{shape}; it cannot have an ABI")));
    }
    if let Some(v) = &sig.variadic {
        return Err(syn::Error::new(v.span(), shape));
    }
    let mut inputs = Vec::new();
    for arg in &sig.inputs {
        match arg {
            FnArg::Typed(t) => inputs.push(&*t.ty),
            FnArg::Receiver(r) => return Err(syn::Error::new(r.span(), format!("{shape}; it cannot take self"))),
        }
    }
    let [ctx, params] = inputs[..] else {
        return Err(syn::Error::new(sig.inputs.span(), format!("{shape}; it takes exactly two arguments")));
    };
    let ReturnType::Type(_, ret) = &sig.output else {
        return Err(syn::Error::new(sig.ident.span(), format!("{shape}; it must return a Result")));
    };
    let result = result_ok_type(ret)
        .ok_or_else(|| syn::Error::new(ret.span(), format!("{shape}; the return type must be `Result<R, E>`")))?;
    Ok(Signature { ctx, params, result, ret })
}

/// `R` of `Result<R, E>` (two type arguments, last segment `Result`).
fn result_ok_type(ty: &Type) -> Option<&Type> {
    if single_arg(ty, "Result").is_some() {
        // `Result<T>` aliases hide the error type; the shape asks for both.
        return None;
    }
    let Type::Path(p) = ty else { return None };
    let last = p.path.segments.last()?;
    if last.ident != "Result" {
        return None;
    }
    let syn::PathArguments::AngleBracketed(args) = &last.arguments else { return None };
    if args.args.len() != 2 {
        return None;
    }
    match args.args.first()? {
        syn::GenericArgument::Type(t) => Some(t),
        _ => None,
    }
}

fn string_lit(e: &Expr) -> Option<String> {
    match e {
        Expr::Lit(ExprLit { lit: Lit::Str(s), .. }) => Some(s.value()),
        _ => None,
    }
}

pub fn expand_command(args: TokenStream, item: TokenStream) -> syn::Result<TokenStream> {
    let f: ItemFn = syn::parse2(item)?;
    let args = parse_args(args)?;
    let sig = signature(&f)?;
    let mut errors = Errors::default();

    // REQ-005: a missing required field fails, naming it.
    for name in REQUIRED {
        if !args.seen.iter().any(|s| s == name) {
            errors.at(Span::call_site(), format!("#[fid_command] is missing required field `{name}`"));
        }
    }
    if let Some(web) = &args.web {
        let note_empty = args.web_note.as_ref().and_then(string_lit).is_some_and(|s| s.is_empty());
        if web != "Available" && (args.web_note.is_none() || note_empty) {
            errors.at(web.span(), format!("#[fid_command] is missing required field `web_note` (web = {web})"));
        }
    }
    if let Some(id) = &args.id {
        if !is_command_name(&id.value()) {
            errors.at(id.span(), "id must be a command name: [a-z0-9_.], starting and ending with a letter or digit");
        }
    }
    if let Some(major) = &args.major {
        if let Err(e) = major.base10_parse::<u32>() {
            errors.push(e);
        }
    }
    for p in args.preconditions.iter().flatten() {
        match p.bound.base10_parse::<u32>() {
            Ok(b) if (1..=MAX_BOUND).contains(&b) => {}
            _ => errors.at(p.bound.span(), "a precondition bound must be an integer 1..=1000000"),
        }
        if !is_command_name(&p.message_key.value()) {
            errors.at(p.message_key.span(), "message_key must match [a-z0-9_.], starting and ending alphanumeric");
        }
    }
    for a in args.algorithms.iter().flatten() {
        if parse_algorithm(&a.value()).is_none() {
            errors.at(a.span(), "an algorithm ID is `module.feature.method@major.minor.patch`");
        }
    }
    if let Some(surfaces) = &args.exposure {
        for (i, s) in surfaces.iter().enumerate() {
            if surfaces[..i].contains(s) {
                errors.at(s.span(), format!("exposure lists `{s}` twice"));
            }
        }
    }
    errors.finish()?;

    let rt = quote!(::fid_registry::decl::rt);
    let name = &f.sig.ident;
    let vis = &f.vis;
    let decl_fn = format_ident!("__fid_command_{}", name.unraw());
    let name_str = name.unraw().to_string();
    let Signature { ctx, params, result, ret } = sig;

    // REQ-004: the attribute's types must equal the signature's.
    let mut checks = Vec::new();
    for (given, actual, what) in [(&args.params, params, "params"), (&args.result, result, "result")] {
        if let Some(given) = given {
            let check = format_ident!("__fid_{}_type_check", what);
            checks.push(quote_spanned! { given.span()=>
                #[allow(dead_code)]
                fn #check(value: #actual) -> #given { value }
            });
        }
    }

    let field = |v: &Option<Ident>, ty: &str| {
        let ty = Ident::new(ty, Span::call_site());
        let v = v.as_ref().map(|i| quote_spanned! { i.span()=> #rt::#ty::#i });
        v.unwrap_or_else(|| quote!(#rt::#ty::None))
    };
    let (id, major) = (&args.id, &args.major);
    let (title, description) = (&args.title, &args.description);
    let side_effect = field(&args.side_effect, "SideEffect");
    let mcp_projection = field(&args.mcp_projection, "Projection");
    let determinism = field(&args.determinism, "Determinism");
    let idempotency = field(&args.idempotency, "Idempotency");
    let fs_effect = field(&args.fs_effect, "FsEffect");
    let undo = field(&args.undo, "Undo");
    let network = field(&args.network, "Network");
    let web = field(&args.web, "Web");
    let preconditions = args.preconditions.iter().flatten().map(|p| {
        let (e, b, k) = (&p.expr, &p.bound, &p.message_key);
        quote! { #rt::Precondition { expr: #rt::string(#e), bound: #b, message_key: #rt::string(#k) } }
    });
    let effects = args.effects.iter().flatten().map(|e| quote_spanned! { e.span()=> #rt::string(&(#e)) });
    let algorithms = args.algorithms.iter().flatten().filter_map(|a| {
        let ([m, fe, me], [x, y, z]) = parse_algorithm(&a.value())?;
        Some(quote! {
            #rt::AlgorithmId {
                module: #rt::string(#m),
                feature: #rt::string(#fe),
                method: #rt::string(#me),
                version: (#x, #y, #z),
            }
        })
    });
    let exposure = args.exposure.iter().flatten().map(|s| quote_spanned! { s.span()=> #rt::Surface::#s });
    let lifecycle = args.lifecycle.iter().flatten().map(|(k, v)| {
        let k = k.unraw().to_string();
        quote_spanned! { v.span()=> (#rt::string(#k), #rt::text(&(#v))) }
    });
    let human_only = args.human_only.as_ref().is_some_and(LitBool::value);
    let cli_bare = args.cli_bare.as_ref().is_some_and(LitBool::value);
    let web_note = match &args.web_note {
        Some(n) => quote_spanned! { n.span()=> ::std::option::Option::Some(#rt::string(&(#n))) },
        None => quote!(::std::option::Option::None),
    };
    let params_schema = quote_spanned! { params.span()=> #rt::schema_of::<#params>() };
    let result_schema = quote_spanned! { result.span()=> #rt::schema_of::<#result>() };

    Ok(quote! {
        #f

        /// The declaration of this command, generated by `#[fid_command]`.
        #[doc(hidden)]
        #vis fn #decl_fn() -> ::fid_registry::decl::CommandDecl {
            #(#checks)*
            ::fid_registry::decl::CommandDecl {
                def: #rt::CommandDef {
                    id: #rt::string(#id),
                    major: #major,
                    title: #rt::string(&(#title)),
                    description: #rt::string(&(#description)),
                    params_schema: #params_schema,
                    result_schema: #result_schema,
                    side_effect: #side_effect,
                    mcp_projection: #mcp_projection,
                    determinism: #determinism,
                    idempotency: #idempotency,
                    fs_effect: #fs_effect,
                    undo: #undo,
                    preconditions: ::std::vec![#(#preconditions),*],
                    effects: ::std::vec![#(#effects),*],
                    algorithms: ::std::vec![#(#algorithms),*],
                    exposure: [#(#exposure),*].into_iter().collect(),
                    lifecycle: [#(#lifecycle),*].into_iter().collect(),
                    human_only: #human_only,
                    network: #network,
                    web: #web,
                    web_note: #web_note,
                    cli_bare: #cli_bare,
                },
                handler: ::fid_registry::decl::HandlerRef::new(
                    #name_str,
                    #name as fn(#ctx, #params) -> #ret,
                ),
            }
        }
    })
}

pub fn expand_list(input: TokenStream) -> syn::Result<TokenStream> {
    let paths = Punctuated::<Path, Token![,]>::parse_terminated.parse2(input)?;
    let mut errors = Errors::default();
    let mut seen: Vec<String> = Vec::new();
    let mut calls = Vec::new();
    for path in &paths {
        let text = quote!(#path).to_string();
        if seen.contains(&text) {
            errors.at(path.span(), format!("`{text}` is listed twice"));
        }
        seen.push(text);
        let mut p = path.clone();
        match p.segments.last_mut() {
            Some(last) if last.arguments.is_empty() => {
                last.ident = format_ident!("__fid_command_{}", last.ident.unraw(), span = last.ident.span());
            }
            _ => errors.at(path.span(), "expected the name of a #[fid_command] handler"),
        }
        calls.push(quote! { #p() });
    }
    errors.finish()?;
    Ok(quote! {
        /// The commands this crate declares, in source order (CORE.REG.DEF.MACRO).
        pub fn declarations() -> ::std::vec::Vec<::fid_registry::decl::CommandDecl> {
            ::std::vec![#(#calls),*]
        }
    })
}
