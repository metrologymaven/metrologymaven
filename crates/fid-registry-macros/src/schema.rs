//! `#[derive(FidSchema)]` (REQ-CORE.REG.DEF.MACRO-001, -002).

use crate::util::{is_option, mentions, snake_case, Errors};
use proc_macro2::{Literal, Span, TokenStream};
use quote::{quote, quote_spanned};
use syn::ext::IdentExt;
use syn::spanned::Spanned;
use syn::{Attribute, Data, DeriveInput, Expr, Fields, Lit, LitBool, LitStr};

/// Largest integer a JSON number holds exactly.
const MAX_EXACT: u64 = 1 << 53;

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
    dimensionless: Option<Span>,
    default: Option<Lit>,
    untrusted: Option<LitBool>,
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
                "dimensionless_unchecked" => f.dimensionless = Some(ident.span()),
                "default" => f.default = Some(meta.value()?.parse()?),
                "untrusted" => f.untrusted = Some(meta.value()?.parse()?),
                other => match EXPR_KEYWORDS.iter().find(|(name, _, _)| *name == other) {
                    Some((_, json, leaf)) => f.keywords.push((json, meta.value()?.parse()?, *leaf)),
                    None => {
                        return Err(meta.error(format!(
                            "unknown #[fid] field attribute `{other}`; expected one of quantity, unit, \
                             dimensionless_unchecked, description, default, minimum, maximum, min_length, \
                             max_length, pattern, min_items, max_items, untrusted, rename"
                        )))
                    }
                },
            }
            Ok(())
        })?;
    }
    Ok(f)
}

/// The JSON value of a `default = LIT`, built at compile time.
fn literal_value(lit: &Lit) -> syn::Result<TokenStream> {
    let rt = quote!(::fid_registry::decl::rt);
    match lit {
        Lit::Bool(b) => Ok(quote! { #rt::CValue::Bool(#b) }),
        Lit::Str(s) => Ok(quote! { #rt::CValue::Str(::std::string::String::from(#s)) }),
        Lit::Int(i) => {
            let v: i64 = i.base10_parse()?;
            if v.unsigned_abs() > MAX_EXACT {
                return Err(syn::Error::new(i.span(), "a default above 2^53 in magnitude is not exact in JSON"));
            }
            // Exact: |v| <= 2^53.
            #[allow(clippy::cast_precision_loss)]
            let f = Literal::f64_suffixed(v as f64);
            Ok(quote! { #rt::CValue::Num(#f) })
        }
        Lit::Float(x) => {
            let v: f64 = x.base10_parse()?;
            if !v.is_finite() {
                return Err(syn::Error::new(x.span(), "a default must be a finite number"));
            }
            let f = Literal::f64_suffixed(v);
            Ok(quote! { #rt::CValue::Num(#f) })
        }
        other => Err(syn::Error::new(other.span(), "a default must be a boolean, integer, float or string literal")),
    }
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
    let own = name.to_string();
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
        if mentions(ty, &["Self", own.as_str()]) {
            errors.at(
                ty.span(),
                format!("field `{fname}` refers to `{own}` itself; recursive schemas are not supported"),
            );
            continue;
        }
        // REQ-002: a float field needs a quantity or the explicit opt-out.
        let float = mentions(ty, &["f64", "f32"]);
        match (&attrs.quantity, attrs.dimensionless) {
            (Some(q), Some(_)) => errors
                .at(q.span(), format!("field `{fname}` has both a quantity and dimensionless_unchecked; keep one")),
            (None, None) if float => errors.at(
                ident.span(),
                format!(
                    "f64 field `{fname}` has no quantity: add #[fid(quantity = \"...\", unit = \"...\")] \
                     or, for a genuinely unitless number, #[fid(dimensionless_unchecked)]"
                ),
            ),
            _ => {}
        }
        if let (Some(u), None) = (&attrs.unit, &attrs.quantity) {
            errors.at(u.span(), format!("field `{fname}` has a unit but no quantity"));
        }
        for lit in [&attrs.quantity, &attrs.unit].into_iter().flatten() {
            if lit.value().is_empty() {
                errors.at(lit.span(), "a quantity or unit must not be empty");
            }
        }
        let default = match attrs.default.as_ref().map(literal_value).transpose() {
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
        let schema = quote_spanned! { ty.span()=> #rt::schema_of::<#ty>() };
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
