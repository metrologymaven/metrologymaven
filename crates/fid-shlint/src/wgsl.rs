//! WGSL resource bindings through the naga WGSL front end (the leaf's Design: "parse WGSL with
//! naga front end for bindings"). Only the front end runs: no validation, no backend, no GPU.

/// A global variable with a `@group(g) @binding(b)` attribute.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct WgslBinding {
    pub group: u32,
    pub binding: u32,
    pub name: Option<String>,
}

/// The resource bindings of one WGSL module, sorted by (group, binding, name). A parse error
/// is returned as (1-based line if known, message); a front-end panic is caught and reported
/// as an error too, so the tool fails closed instead of crashing.
pub fn bindings(source: &str) -> Result<Vec<WgslBinding>, (Option<u32>, String)> {
    let parsed = std::panic::catch_unwind(|| naga::front::wgsl::parse_str(source));
    let module = match parsed {
        Ok(Ok(module)) => module,
        Ok(Err(e)) => {
            let line = e.location(source).map(|l| l.line_number);
            return Err((line, e.message().to_string()));
        }
        Err(_) => return Err((None, "the WGSL front end stopped unexpectedly".to_string())),
    };
    let mut out: Vec<WgslBinding> = module
        .global_variables
        .iter()
        .filter_map(|(_, var)| {
            var.binding.as_ref().map(|rb| WgslBinding { group: rb.group, binding: rb.binding, name: var.name.clone() })
        })
        .collect();
    out.sort();
    Ok(out)
}
