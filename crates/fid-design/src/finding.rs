//! Findings and bounded file reading shared by the `princ` and `words` checks.

use fid_canon::CValue;
use std::collections::BTreeMap;
use std::fmt;
use std::io::Read;
use std::path::Path;

/// One finding: a stable code, the subject it names and a human-readable detail.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Finding {
    pub code: &'static str,
    pub subject: String,
    pub detail: String,
}

impl Finding {
    pub fn new(code: &'static str, subject: impl Into<String>, detail: impl Into<String>) -> Self {
        Finding { code, subject: subject.into(), detail: detail.into() }
    }
}

impl fmt::Display for Finding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}: {}", self.code, self.subject, self.detail)
    }
}

/// Findings as a JSON array of `{code, subject, detail}`.
pub fn findings_value(f: &[Finding]) -> CValue {
    CValue::Arr(
        f.iter()
            .map(|x| {
                let mut m = BTreeMap::new();
                m.insert("code".to_string(), CValue::Str(x.code.to_string()));
                m.insert("subject".to_string(), CValue::Str(x.subject.clone()));
                m.insert("detail".to_string(), CValue::Str(x.detail.clone()));
                CValue::Obj(m)
            })
            .collect(),
    )
}

/// Appends one line per finding and a summary line.
pub fn findings_text(out: &mut String, f: &[Finding]) {
    for x in f {
        out.push_str(&x.to_string());
        out.push('\n');
    }
    if f.is_empty() {
        out.push_str("ok: no findings\n");
    } else {
        out.push_str(&format!("{} finding(s)\n", f.len()));
    }
}

/// Reads a UTF-8 text file of at most `max` bytes. A leading UTF-8 byte-order mark is dropped.
/// Errors (exit code 2): unreadable, too large, not UTF-8.
pub fn read_text(path: &Path, max: u64) -> Result<String, String> {
    let file = std::fs::File::open(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let mut buf = Vec::new();
    file.take(max.saturating_add(1))
        .read_to_end(&mut buf)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    if buf.len() as u64 > max {
        return Err(format!("{} is larger than {max} bytes", path.display()));
    }
    let s = String::from_utf8(buf).map_err(|_| format!("{} is not UTF-8", path.display()))?;
    Ok(match s.strip_prefix('\u{feff}') {
        Some(rest) => rest.to_string(),
        None => s,
    })
}
