//! `#[derive(FidSchema)]` (REQ-CORE.REG.DEF.MACRO-001, -002).

use crate::util::{is_option, mentions, snake_case, Errors};
use proc_macro2::TokenStream;
use quote::{quote, quote_spanned};
use syn::ext::IdentExt;
use syn::spanned::Spanned;
use syn::visit::Visit;
use syn::{
    Attribute, Data, DeriveInput, Expr, ExprArray, ExprLit, ExprUnary, Fields, Ident, Lit, LitBool, LitStr, Token,
    Type, TypePath, UnOp,
};

pub fn expand(input: TokenStream) -> syn::Result<TokenStream> {
    let input: DeriveInput = syn::parse2(input)?;
    if !input.generics.params.is_empty() || input.generics.where_clause.is_some() {
        return Err(syn::Error::new(
            input.generics.span(),
            "FidSchema cannot be derived for a generic type: a command schema is one fixed document",
        ));
    }
    let name = &input.ident;
    let container = container_attrs(&input.attrs)?;
    let body = match &input.data {
        Data::Struct(s) => struct_body(name, &s.fields, &container)?,
        Data::Enum(e) => enum_body(e, &container)?,
        Data::Union(u) => {
            return Err(syn::Error::new(u.union_token.span, "FidSchema cannot be derived for a union"));
        }
    };
    Ok(quote! {
        impl ::fid_registry::decl::FidSchema for #name {
            fn schema() -> ::fid_registry::decl::rt::CValue {
                #body
            }
        }
    })
}

/// `#[fid(title = .., description = ..)]` on the type.
#[derive(Default)]
struct Container {
    keywords: Vec<(&'static str, Expr)>,
}

fn container_attrs(attrs: &[Attribute]) -> syn::Result<Container> {
    let mut c = Container::default();
    for attr in attrs.iter().filter(|a| a.path().is_ident("fid")) {
        attr.parse_nested_meta(|meta| {
            let key = if meta.path.is_ident("title") {
                "title"
            } else if meta.path.is_ident("description") {
                "description"
            } else {
                return Err(meta.error("unknown #[fid] attribute on a type; expected `title` or `description`"));
            };
            if c.keywords.iter().any(|(k, _)| *k == key) {
                return Err(meta.error(format!("`{key}` is given twice")));
            }
            c.keywords.push((key, meta.value()?.parse()?));
            Ok(())
        })?;
    }
    Ok(c)
}

fn container_tokens(c: &Container, target: TokenStream) -> TokenStream {
    let sets = c.keywords.iter().map(|(k, e)| {
        quote! { ::fid_registry::decl::rt::set(&mut #target, #k, ::fid_registry::decl::rt::text(&(#e))); }
    });
    quote! { #(#sets)* }
}

/// Parsed `#[fid(...)]` of one field.
#[derive(Default)]
struct FieldAttrs {
    rename: Option<LitStr>,
    quantity: Option<LitStr>,
    unit: Option<LitStr>,
    default: Option<Expr>,
    untrusted: Option<LitBool>,
    /// `path_like` (a flag): the field is a file handle (`"path_like": true`, CORE.BUS.POL.FS).
    path_like: Option<proc_macro2::Span>,
    /// `extensions = ["step", "stl"]`: `x-extensions`, checked by [`extension_list`].
    extensions: Option<(proc_macro2::Span, Vec<String>)>,
    /// (keyword, value expression, applies to the element schema).
    keywords: Vec<(&'static str, Expr, bool)>,
}

/// Attributes whose value is an expression: (attribute, JSON keyword, applies to the element
/// schema of an array).
const EXPR_KEYWORDS: [(&str, &str, bool); 8] = [
    ("description", "description", false),
    ("minimum", "minimum", true),
    ("maximum", "maximum", true),
    ("min_length", "minLength", true),
    ("max_length", "maxLength", true),
    ("pattern", "pattern", true),
    ("min_items", "minItems", false),
    ("max_items", "maxItems", false),
];

fn field_attrs(attrs: &[Attribute]) -> syn::Result<FieldAttrs> {
    let mut f = FieldAttrs::default();
    let mut seen: Vec<String> = Vec::new();
    for attr in attrs.iter().filter(|a| a.path().is_ident("fid")) {
        attr.parse_nested_meta(|meta| {
            let Some(ident) = meta.path.get_ident() else { return Err(meta.error("expected a #[fid] key")) };
            let key = ident.to_string();
            if seen.contains(&key) {
                return Err(meta.error(format!("`{key}` is given twice")));
            }
            seen.push(key.clone());
            match key.as_str() {
                "rename" => f.rename = Some(meta.value()?.parse()?),
                "quantity" => f.quantity = Some(meta.value()?.parse()?),
                "unit" => f.unit = Some(meta.value()?.parse()?),
                "default" => f.default = Some(meta.value()?.parse()?),
                "untrusted" => f.untrusted = Some(meta.value()?.parse()?),
                "path_like" => {
                    if meta.input.peek(Token![=]) || meta.input.peek(syn::token::Paren) {
                        return Err(meta.error("`path_like` is a flag and takes no value"));
                    }
                    f.path_like = Some(meta.path.span());
                }
                "extensions" => {
                    let span = meta.path.span();
                    let list: ExprArray = meta.value()?.parse()?;
                    f.extensions = Some((span, extension_list(&list)?));
                }
                other => match EXPR_KEYWORDS.iter().find(|(name, _, _)| *name == other) {
                    Some((_, json, leaf)) => f.keywords.push((json, meta.value()?.parse()?, *leaf)),
                    None => {
                        return Err(meta.error(format!(
                            "unknown #[fid] field attribute `{other}`; expected one of quantity, unit, \
                             description, default, minimum, maximum, min_length, \
                             max_length, pattern, min_items, max_items, untrusted, path_like, extensions, rename"
                        )))
                    }
                },
            }
            Ok(())
        })?;
    }
    Ok(f)
}

/// The extensions of `extensions = [...]`: a non-empty array of string literals, each 1 to 16
/// lowercase ASCII letters or digits without the dot, sorted and distinct (the
/// `x-extensions` rule of CORE.REG.DEF.SCHEMA), so the schema is exactly what is written.
fn extension_list(list: &ExprArray) -> syn::Result<Vec<String>> {
    if list.elems.is_empty() {
        return Err(syn::Error::new(list.span(), "`extensions` must list at least one extension"));
    }
    if list.elems.len() > 64 {
        return Err(syn::Error::new(list.span(), "`extensions` lists at most 64 extensions"));
    }
    let mut out: Vec<String> = Vec::new();
    for e in &list.elems {
        let Expr::Lit(ExprLit { lit: Lit::Str(lit), .. }) = e else {
            return Err(syn::Error::new(e.span(), "an extension must be a string literal such as \"stl\""));
        };
        let ext = lit.value();
        let valid = (1..=16).contains(&ext.len()) && ext.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit());
        if !valid {
            return Err(syn::Error::new(
                lit.span(),
                format!("{ext:?} is not a valid extension: 1 to 16 lowercase ASCII letters or digits, without the dot"),
            ));
        }
        if out.last().is_some_and(|prev| prev.as_str() >= ext.as_str()) {
            return Err(syn::Error::new(lit.span(), "`extensions` must be distinct and in sorted order"));
        }
        out.push(ext);
    }
    Ok(out)
}

/// The value of a `default = LIT` (an optional `-` before a numeric literal), checked against
/// the field type by the compiler through `rt::AcceptsDefault<kind>` (maintainer decision on
/// #262): the literal is typed as the field's `Value`, so a wrong kind or an out-of-range value
/// does not compile.
fn default_value(ty: &Type, expr: &Expr) -> syn::Result<TokenStream> {
    let rt = quote!(::fid_registry::decl::rt);
    let not_literal = "a default must be a boolean, integer, float or string literal";
    let (negative, lit) = match expr {
        Expr::Lit(ExprLit { lit, .. }) => (false, lit),
        Expr::Unary(ExprUnary { op: UnOp::Neg(_), expr: inner, .. }) => match &**inner {
            Expr::Lit(ExprLit { lit: lit @ (Lit::Int(_) | Lit::Float(_)), .. }) => (true, lit),
            other => return Err(syn::Error::new(other.span(), "`-` applies only to a numeric default literal")),
        },
        other => return Err(syn::Error::new(other.span(), not_literal)),
    };
    let kind = match lit {
        Lit::Bool(_) if !negative => quote!(BoolLit),
        Lit::Str(_) if !negative => quote!(StrLit),
        Lit::Int(i) if i.suffix().is_empty() => quote!(IntLit),
        Lit::Float(f) if f.suffix().is_empty() => quote!(FloatLit),
        Lit::Int(_) | Lit::Float(_) => {
            return Err(syn::Error::new(lit.span(), "a default literal takes no type suffix; the field type decides"))
        }
        other => return Err(syn::Error::new(other.span(), not_literal)),
    };
    Ok(quote_spanned! { expr.span()=>
        <#ty as #rt::AcceptsDefault<#rt::#kind>>::default_value(#expr)
    })
}

/// Whether `ty` names the type being derived: bare `Self`, or the type's own name as a
/// single-segment path, anywhere in `ty`. A path such as `other::Foo` is a different type and
/// is not refused. Mutual recursion through another type cannot be seen here (NODE.md).
fn refers_to_self(ty: &Type, own: &Ident) -> bool {
    struct Finder<'a> {
        own: &'a Ident,
        found: bool,
    }
    impl<'ast> Visit<'ast> for Finder<'_> {
        fn visit_type_path(&mut self, p: &'ast TypePath) {
            if p.qself.is_none() && p.path.leading_colon.is_none() && p.path.segments.len() == 1 {
                let id = &p.path.segments[0].ident;
                if id == "Self" || id == self.own {
                    self.found = true;
                }
            }
            syn::visit::visit_type_path(self, p);
        }
    }
    let mut f = Finder { own, found: false };
    f.visit_type(ty);
    f.found
}

fn struct_body(name: &syn::Ident, fields: &Fields, container: &Container) -> syn::Result<TokenStream> {
    let rt = quote!(::fid_registry::decl::rt);
    let named = match fields {
        Fields::Named(n) => &n.named,
        Fields::Unit => {
            let sets = container_tokens(container, quote!(__fid_schema));
            return Ok(quote! {
                let mut __fid_schema = #rt::Object::new().finish();
                #sets
                __fid_schema
            });
        }
        Fields::Unnamed(u) => {
            return Err(syn::Error::new(
                u.span(),
                "FidSchema needs named fields (a tuple struct has no JSON property names)",
            ));
        }
    };
    let mut errors = Errors::default();
    let mut names: Vec<String> = Vec::new();
    let mut adds = Vec::new();
    for field in named {
        let Some(ident) = &field.ident else { continue };
        let attrs = match field_attrs(&field.attrs) {
            Ok(a) => a,
            Err(e) => {
                errors.push(e);
                continue;
            }
        };
        let fname = ident.unraw().to_string();
        let json_name = attrs.rename.as_ref().map(LitStr::value).unwrap_or_else(|| fname.clone());
        if json_name.is_empty() || !json_name.bytes().all(|b| b.is_ascii_graphic()) {
            errors.at(ident.span(), format!("property name {json_name:?} must be non-empty printable ASCII"));
        }
        if names.contains(&json_name) {
            errors.at(ident.span(), format!("property `{json_name}` is declared twice"));
        }
        names.push(json_name.clone());
        let ty = &field.ty;
        if refers_to_self(ty, name) {
            errors.at(
                ty.span(),
                format!("field `{fname}` refers to `{name}` itself; recursive schemas are not supported"),
            );
            continue;
        }
        // REQ-002: a float field needs a quantity. This check only gives a clear message for a
        // spelled-out f64; the types enforce the rule (f64 has no FidSchema, only FidNumber),
        // so an alias of f64 cannot slip through.
        if attrs.quantity.is_none() && mentions(ty, &["f64", "f32"]) {
            errors.at(
                ident.span(),
                format!(
                    "f64 field `{fname}` has no quantity: add #[fid(quantity = \"...\", unit = \"...\")] \
                     (quantity = \"dimensionless\" for a pure ratio)"
                ),
            );
        }
        if let (Some(u), None) = (&attrs.unit, &attrs.quantity) {
            errors.at(u.span(), format!("field `{fname}` has a unit but no quantity"));
        }
        if let (Some((span, _)), None) = (&attrs.extensions, &attrs.path_like) {
            errors.at(*span, format!("field `{fname}`: `extensions` needs `path_like`"));
        }
        if let (Some(span), Some(_)) = (&attrs.path_like, &attrs.quantity) {
            errors.at(*span, format!("field `{fname}`: a path_like field is a string and cannot carry a quantity"));
        }
        for lit in [&attrs.quantity, &attrs.unit].into_iter().flatten() {
            if lit.value().is_empty() {
                errors.at(lit.span(), "a quantity or unit must not be empty");
            }
        }
        let default = match attrs.default.as_ref().map(|d| default_value(ty, d)).transpose() {
            Ok(d) => d,
            Err(e) => {
                errors.push(e);
                None
            }
        };
        let required = !is_option(ty) && default.is_none();
        let mut sets = Vec::new();
        if let Some(q) = &attrs.quantity {
            sets.push(quote! { #rt::set_leaf(&mut __fid_field, "x-quantity", #rt::text(#q)); });
        }
        if let Some(u) = &attrs.unit {
            sets.push(quote! { #rt::set_leaf(&mut __fid_field, "x-unit", #rt::text(#u)); });
        }
        if let Some((_, exts)) = &attrs.extensions {
            sets.push(quote! {
                #rt::set(&mut __fid_field, "x-extensions", #rt::CValue::Arr(::std::vec![#(#rt::text(#exts)),*]));
            });
        }
        if let Some(b) = &attrs.untrusted {
            sets.push(quote! { #rt::set_leaf(&mut __fid_field, "x-untrusted", #rt::CValue::Bool(#b)); });
        }
        for (json, expr, leaf) in &attrs.keywords {
            let value = match *json {
                "description" | "pattern" => quote_spanned! { expr.span()=> #rt::text(&(#expr)) },
                _ => quote_spanned! { expr.span()=> #rt::number(#expr) },
            };
            let setter = if *leaf { quote!(set_leaf) } else { quote!(set) };
            sets.push(quote! { #rt::#setter(&mut __fid_field, #json, #value); });
        }
        if let Some(d) = default {
            sets.push(quote! { #rt::set(&mut __fid_field, "default", #d); });
        }
        // A field with a quantity is a number: FidNumber, never FidSchema (REQ-002). A path_like
        // field is a String or an Option<String> (PathLikeField).
        let schema = if attrs.path_like.is_some() {
            quote_spanned! { ty.span()=> #rt::path_like_schema_of::<#ty>() }
        } else if attrs.quantity.is_some() {
            quote_spanned! { ty.span()=> #rt::number_schema_of::<#ty>() }
        } else {
            quote_spanned! { ty.span()=> #rt::schema_of::<#ty>() }
        };
        let mutability = if sets.is_empty() { quote!() } else { quote!(mut) };
        adds.push(quote! {
            {
                let #mutability __fid_field = #schema;
                #(#sets)*
                __fid_object.field(#json_name, __fid_field, #required);
            }
        });
    }
    errors.finish()?;
    let sets = container_tokens(container, quote!(__fid_schema));
    let object = if adds.is_empty() {
        quote! { #rt::Object::new() }
    } else {
        quote! {{
            let mut __fid_object = #rt::Object::new();
            #(#adds)*
            __fid_object
        }}
    };
    let mutability = if container.keywords.is_empty() { quote!() } else { quote!(mut) };
    Ok(quote! {
        let #mutability __fid_schema = #object.finish();
        #sets
        __fid_schema
    })
}

fn enum_body(e: &syn::DataEnum, container: &Container) -> syn::Result<TokenStream> {
    let rt = quote!(::fid_registry::decl::rt);
    if e.variants.is_empty() {
        return Err(syn::Error::new(e.enum_token.span, "FidSchema needs at least one variant"));
    }
    let mut errors = Errors::default();
    let mut values: Vec<String> = Vec::new();
    for v in &e.variants {
        if !matches!(v.fields, Fields::Unit) {
            errors.at(v.span(), "FidSchema enums map to JSON strings, so every variant must be a unit variant");
            continue;
        }
        let mut rename: Option<LitStr> = None;
        for attr in v.attrs.iter().filter(|a| a.path().is_ident("fid")) {
            if let Err(err) = attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("rename") && rename.is_none() {
                    rename = Some(meta.value()?.parse()?);
                    Ok(())
                } else {
                    Err(meta.error("the only #[fid] attribute on a variant is one `rename`"))
                }
            }) {
                errors.push(err);
            }
        }
        let value = rename.map(|r| r.value()).unwrap_or_else(|| snake_case(&v.ident.unraw().to_string()));
        if value.is_empty() {
            errors.at(v.span(), "an enum value must not be empty");
        }
        if values.contains(&value) {
            errors.at(v.span(), format!("enum value `{value}` is used twice"));
        }
        values.push(value);
    }
    errors.finish()?;
    let sets = container_tokens(container, quote!(__fid_schema));
    let mutability = if container.keywords.is_empty() { quote!() } else { quote!(mut) };
    Ok(quote! {
        let #mutability __fid_schema = #rt::string_enum(&[#(#values),*]);
        #sets
        __fid_schema
    })
}
