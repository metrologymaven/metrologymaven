//! Command declaration macros of Metrology Maven (spec node `CORE.REG.DEF.MACRO`).
//!
//! Use them through `fid-registry`, which re-exports them next to the runtime support they
//! expand to (`fid_registry::decl`). The generated code names `::fid_registry`, so a crate
//! that uses the macros depends on `fid-registry`.
//!
//! - `#[derive(FidSchema)]` generates a JSON Schema 2020-12 document from a struct or a
//!   unit-only enum (REQ-001, REQ-002).
//! - `#[fid_command(...)]` on a handler `fn(Ctx, P) -> Result<R, E>` generates the command's
//!   declaration with the schemas of `P` and `R` (REQ-003 to REQ-005).
//! - `fid_commands![...]` generates `pub fn declarations()` in source order (REQ-006).
//!
//! Every problem is a compile error (`syn::Error`); the macros never panic, and the code they
//! generate contains no `unsafe`, no static constructor and no linker-section attribute
//! (REQ-006, AC-4). The expansion is a pure function of the input tokens, with no hash maps,
//! clock or environment, so two builds generate the same code and the same schemas (REQ-007).

use proc_macro::TokenStream;

mod command;
mod schema;
mod util;

/// Generates `fid_registry::decl::FidSchema` for a struct with named fields, a unit struct
/// or an enum whose variants are all units.
///
/// Field attributes, all inside `#[fid(...)]`:
///
/// | Attribute | Effect |
/// |---|---|
/// | `quantity = "length"`, `unit = "mm"` | `x-quantity`, `x-unit` |
/// | `dimensionless_unchecked` | lets an `f64` field go without a quantity |
/// | `description = EXPR` | `description` (any `&str` expression) |
/// | `default = LIT` | `default`; the field is then not `required` |
/// | `minimum = EXPR`, `maximum = EXPR` | numeric bounds |
/// | `min_length = EXPR`, `max_length = EXPR`, `pattern = EXPR` | string limits |
/// | `min_items = EXPR`, `max_items = EXPR` | array limits |
/// | `untrusted = BOOL` | `x-untrusted` |
/// | `rename = "name"` | the JSON property name |
///
/// Value keywords (quantity, unit, bounds, string limits, `x-untrusted`) apply to the element
/// schema of an array field; the others to the field's own schema. An `Option<T>` field is not
/// `required`. On the type: `#[fid(title = EXPR, description = EXPR)]`; on an enum variant:
/// `#[fid(rename = "name")]` (default: the variant name in snake case).
#[proc_macro_derive(FidSchema, attributes(fid))]
pub fn derive_fid_schema(input: TokenStream) -> TokenStream {
    schema::expand(input.into()).unwrap_or_else(syn::Error::into_compile_error).into()
}

/// Declares a command handler `fn(ctx: Ctx, params: P) -> Result<R, E>`.
///
/// Required: `id = "domain.name"`, `major = N`, `title`, `description`, `side_effect`,
/// `mcp_projection`, `determinism`, `idempotency`, `fs_effect`, `undo`, `preconditions = [...]`,
/// `effects = [...]`, `algorithms = [...]`, `exposure = [...]`, `lifecycle(...)`, `web`, and
/// `web_note` when `web` is not `Available`. Optional: `human_only`, `network`, `cli_bare`, and
/// `params = Type` / `result = Type`, which must equal the signature's types. Class and enum
/// values are the variant names of `fid_registry::def` (`side_effect = W2`, `mcp_projection =
/// RequestOnly`, `exposure = [Cli, Mcp]`). A precondition is `(expr = "...", bound = N,
/// message_key = "...")`.
#[proc_macro_attribute]
pub fn fid_command(args: TokenStream, item: TokenStream) -> TokenStream {
    let item = proc_macro2::TokenStream::from(item);
    match command::expand_command(args.into(), item.clone()) {
        Ok(out) => out.into(),
        // Keep the handler itself, so the only errors are the declaration's.
        Err(e) => {
            let mut out = e.into_compile_error();
            out.extend(item);
            out.into()
        }
    }
}

/// `fid_commands![handler_a, module::handler_b]` generates
/// `pub fn declarations() -> Vec<fid_registry::decl::CommandDecl>` listing the handlers'
/// declarations in the order written.
#[proc_macro]
pub fn fid_commands(input: TokenStream) -> TokenStream {
    command::expand_list(input.into()).unwrap_or_else(syn::Error::into_compile_error).into()
}

#[cfg(test)]
mod tests;
