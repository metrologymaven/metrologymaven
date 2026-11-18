//! `qif-norm`: when two QIF-XML documents count as equal (spec node `IO.QIF.NORM`).
//!
//! Exact canonical-XML equality is not claimed. Two documents are equal under a qif-norm
//! version when [`normalise`] gives the same bytes for both; [`compare`] walks the same
//! canonical stream and reports the first difference. The rule list is the data file
//! `qif-norm.toml` ([`RULE_FILE`]); its version is locked by digest in `qif-norm.lock`
//! ([`LOCK_FILE`], [`check_version`]).
//!
//! # The rules of `qif-norm@1` (REQ-IO.QIF.NORM-001)
//!
//! Input is read by the hardened `fid-xml` reader: no DTD, no entities other than the
//! five predefined ones, no XInclude, bounded by a [`fid_io_reg::Budget`]. Reader errors
//! (`malformed`, `truncated_file`, `xml_forbidden_construct`, `limit_exceeded`, ...) are
//! returned unchanged. The output is exactly:
//!
//! 1. **`c14n11`, Canonical XML 1.1 form.** UTF-8 without BOM and without XML
//!    declaration. Comments are removed. CDATA sections become ordinary escaped text, and
//!    adjacent text (including text separated only by comments or CDATA boundaries) is
//!    one text node. Empty elements are written as a start and end tag (`<a></a>`).
//!    Attributes are sorted by namespace name (none first), then local name, and written
//!    as `name="value"` with one space before each. Text escapes `&` `<` `>` and CR
//!    (`&#xD;`); attribute values escape `&` `<` `"` tab (`&#x9;`) LF (`&#xA;`) and CR
//!    (`&#xD;`). Processing instructions are kept as `<?target data?>`; outside the root
//!    element they are followed (before the root) or preceded (after it) by one LF.
//!    Whitespace outside the root element is removed. Every other character is written
//!    as itself; character references are resolved.
//! 2. **`xsd-collapse`.** The text content of elements, and the values of attributes,
//!    listed in `[simple_content]` of the rule file (XSD list and non-string atomic
//!    simple types) is collapsed: tab, LF and CR become spaces, runs of spaces become one,
//!    leading and trailing spaces go. Text that collapses to nothing is removed.
//!    `xsi:type` values are always collapsed (they are QNames). Nothing else is
//!    rewritten: numeric lexical forms are never changed (`1.50` stays `1.50`,
//!    REQ-IO.QIF.NORM-004), and no number is ever parsed, so there is no floating point.
//! 3. **`drop-ws-text`.** In an element that has at least one child element, text nodes
//!    that hold only whitespace (space, tab, LF, CR) are removed. In an element without
//!    child elements, text is kept as it is (subject to rule 2).
//! 4. **`ns-prefix-table`.** Every namespace-qualified element and attribute name gets a
//!    prefix chosen by namespace name: the fixed `[prefixes]` table first; `xml` for the
//!    XML namespace; otherwise `n1`, `n2`, ... in order of first use in document order,
//!    where the namespaces one start tag uses are taken in sorted order. The default
//!    namespace is never used. A namespace declaration `xmlns:p="uri"` is written on each
//!    element that uses the namespace in its name, an attribute name or an `xsi:type`
//!    value and has no output ancestor that already declares it; declarations come
//!    first in the tag, sorted by prefix. Unused declarations are removed. `xsi:type`
//!    values are resolved against the input's in-scope namespaces and rewritten as
//!    `p:local` (or `local` when not in a namespace); a value with an unbound prefix is
//!    kept as collapsed.
//!
//! The rules are deterministic (D0: no floating point, no hash maps, no locale),
//! idempotent (`normalise(normalise(x)) == normalise(x)`) and bounded: the reader
//! budget bounds input, depth and time; held-back content and output are bounded by the
//! budget's allocation limit (`limit_exceeded`).
//!
//! Known limits of version 1: which elements are simple-typed comes from an interim,
//! hand-written table until `IO.QIF.GEN` generates one; QName-valued content other than
//! `xsi:type` is not rewritten.

mod compare;
mod error;
mod rules;
mod stream;
mod version;

pub use compare::{compare_with, Diff, Equality};
pub use error::NormError;
pub use rules::{NormRules, LOCK_FILE, RULE_FILE, RULE_IDS};
pub use version::{check_version, rule_file_digest};

use fid_io_reg::{BoundedSource, Budget, IoError, LimitKind, LimitRequest, Position, Principal};
use stream::{Stream, Writer};

/// The default budget: the harness defaults, as a script principal (limits can only be
/// lowered from the defaults).
pub fn default_budget() -> Result<Budget, NormError> {
    Ok(Budget::new(Principal::Script, &LimitRequest::default())?)
}

/// Normalises `xml` under the shipped rules and the default budget.
pub fn normalise(xml: &[u8]) -> Result<Vec<u8>, NormError> {
    normalise_with(xml, default_budget()?, &NormRules::builtin()?)
}

/// Normalises `xml` under `rules`, reading under `budget`.
pub fn normalise_with(xml: &[u8], budget: Budget, rules: &NormRules) -> Result<Vec<u8>, NormError> {
    let mut stream = Stream::new(BoundedSource::new(xml, budget), rules, false);
    let cap = stream.cap();
    let mut writer = Writer::default();
    while let Some(item) = stream.next_item()? {
        writer.write(&item.kind);
        if writer.out.len() as u64 > cap {
            let at = Position::byte(xml.len() as u64);
            return Err(NormError::Harness(IoError::LimitExceeded { limit: LimitKind::Alloc, value: cap, at }));
        }
    }
    Ok(writer.out)
}

/// Compares two documents under the shipped rules, each with the default budget.
pub fn compare(a: &[u8], b: &[u8]) -> Result<Equality, NormError> {
    compare_with(a, b, default_budget()?, default_budget()?, &NormRules::builtin()?)
}
