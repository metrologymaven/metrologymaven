//! Redaction of identifying strings (REQ-PLAT.DOCTOR.REPORT-003).
//!
//! [`Redactor::redact_text`] applies three passes to a string, in this order; text already
//! replaced by a placeholder is never scanned again:
//!
//! 1. **Paths.** An absolute path starts where the previous character is absent or is not
//!    alphanumeric and not one of `. _ - ~ / \`, and is a drive path (`C:\`, `C:/`), or a `/`
//!    or `\` followed by a character that does not end a path (so `\\server\share` and rooted
//!    `\dir` count too). It runs until whitespace, a control or format character, or one of
//!    ``" ' ` < > |``; when the install directory or the home directory starts there, the path
//!    runs at least to its end (so a directory name with spaces is taken whole). Trailing
//!    `. , ; : ! ? ) ] }` are left outside the path. A path under the install directory
//!    (component-wise prefix; ASCII case-insensitive and `/` = `\` only for a Windows-style
//!    install directory) with no `..` component is kept; every other path becomes
//!    [`PLACEHOLDER_PATH`].
//! 2. **Home.** Every occurrence of the home directory (ASCII case-insensitive, `/` = `\`)
//!    that is not followed by a path character (alphanumeric, `. _ - ~`) becomes
//!    [`PLACEHOLDER_HOME`]. After pass 1 this only happens inside kept install paths and in
//!    text that is not a path.
//! 3. **Names.** Every occurrence of the user name and the host name (and the host's first
//!    DNS label, when the name has a dot), longest first, that is ASCII case-insensitively
//!    equal and has no alphanumeric character on either side becomes [`PLACEHOLDER_USER`] or
//!    [`PLACEHOLDER_HOST`].
//!
//! Every comparison is per Unicode scalar value; only ASCII letters fold case.

use super::{fit_detail, ReportError};
use crate::probe::{Arch, EnvSnapshot, Probed};

/// Replaces the current user name.
pub const PLACEHOLDER_USER: &str = "<redacted:user>";
/// Replaces the host name.
pub const PLACEHOLDER_HOST: &str = "<redacted:host>";
/// Replaces the home directory.
pub const PLACEHOLDER_HOME: &str = "<redacted:home>";
/// Replaces an absolute path outside the install directory.
pub const PLACEHOLDER_PATH: &str = "<redacted:path>";

/// Longest host name read from the OS, in bytes (RFC 1035 limit plus margin).
const MAX_HOST_BYTES: usize = 255;

/// The identifying strings of the current session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identity {
    pub user: String,
    pub host: String,
    /// Absolute home directory.
    pub home: String,
}

impl Identity {
    /// Gathers the user name, host name and home directory from the OS at call time.
    /// Fails with [`ReportError::IdentityUnavailable`] when one cannot be determined, since
    /// redaction could not then be guaranteed.
    pub fn from_os() -> Result<Identity, ReportError> {
        let home = os_home()?;
        let user = os_user(&home)?;
        let host = os_host()?;
        Ok(Identity { user, host, home })
    }
}

fn env_nonempty(name: &str, item: &'static str) -> Result<Option<String>, ReportError> {
    match std::env::var(name) {
        Ok(v) if !v.trim().is_empty() => Ok(Some(v.trim().to_string())),
        Ok(_) | Err(std::env::VarError::NotPresent) => Ok(None),
        Err(std::env::VarError::NotUnicode(_)) => Err(ReportError::IdentityUnavailable { item }),
    }
}

fn first_env(names: &[&str], item: &'static str) -> Result<Option<String>, ReportError> {
    for n in names {
        if let Some(v) = env_nonempty(n, item)? {
            return Ok(Some(v));
        }
    }
    Ok(None)
}

fn os_home() -> Result<String, ReportError> {
    #[cfg(windows)]
    let found = match first_env(&["USERPROFILE"], "home")? {
        Some(h) => Some(h),
        None => match (env_nonempty("HOMEDRIVE", "home")?, env_nonempty("HOMEPATH", "home")?) {
            (Some(d), Some(p)) => Some(format!("{d}{p}")),
            _ => None,
        },
    };
    #[cfg(not(windows))]
    let found = first_env(&["HOME"], "home")?;
    match found {
        Some(h) if is_absolute(&h.chars().collect::<Vec<_>>()) => Ok(h),
        _ => Err(ReportError::IdentityUnavailable { item: "home" }),
    }
}

fn os_user(home: &str) -> Result<String, ReportError> {
    #[cfg(windows)]
    let names: &[&str] = &["USERNAME"];
    #[cfg(not(windows))]
    let names: &[&str] = &["USER", "LOGNAME"];
    if let Some(u) = first_env(names, "user")? {
        return Ok(u);
    }
    // Fall back to the home directory's last component (e.g. containers without `USER`).
    let last = home.trim_end_matches(['/', '\\']).rsplit(['/', '\\']).next().unwrap_or("").trim();
    if last.is_empty() {
        Err(ReportError::IdentityUnavailable { item: "user" })
    } else {
        Ok(last.to_string())
    }
}

fn host_ok(s: &str) -> Option<String> {
    let t = s.lines().next().unwrap_or("").trim();
    if t.is_empty() || t.len() > MAX_HOST_BYTES || t.chars().any(char::is_control) {
        None
    } else {
        Some(t.to_string())
    }
}

#[cfg(not(windows))]
fn read_small(path: &str) -> Option<String> {
    use std::io::Read;
    let f = std::fs::File::open(path).ok()?;
    let mut buf = Vec::new();
    f.take(MAX_HOST_BYTES as u64 + 2).read_to_end(&mut buf).ok()?;
    String::from_utf8(buf).ok().and_then(|s| host_ok(&s))
}

fn os_host() -> Result<String, ReportError> {
    #[cfg(windows)]
    {
        if let Some(h) = first_env(&["COMPUTERNAME"], "host")?.and_then(|h| host_ok(&h)) {
            return Ok(h);
        }
    }
    #[cfg(not(windows))]
    {
        for p in ["/proc/sys/kernel/hostname", "/etc/hostname"] {
            if let Some(h) = read_small(p) {
                return Ok(h);
            }
        }
        if let Ok(out) = std::process::Command::new("hostname").output() {
            if out.status.success() && out.stdout.len() <= MAX_HOST_BYTES + 2 {
                if let Some(h) = String::from_utf8(out.stdout).ok().and_then(|s| host_ok(&s)) {
                    return Ok(h);
                }
            }
        }
    }
    match first_env(&["HOSTNAME"], "host")?.and_then(|h| host_ok(&h)) {
        Some(h) => Ok(h),
        None => Err(ReportError::IdentityUnavailable { item: "host" }),
    }
}

// ------------------------------------------------------------------ character classes

fn is_sep(c: char) -> bool {
    c == '/' || c == '\\'
}

/// Ends a path token.
fn is_term(c: char) -> bool {
    c.is_whitespace() || super::is_escaped_char(c) || matches!(c, '"' | '\'' | '`' | '<' | '>' | '|')
}

/// May continue a name or path component.
fn is_path_char(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '.' | '_' | '-' | '~')
}

fn fold_eq(a: char, b: char) -> bool {
    a == b || (a.is_ascii() && b.is_ascii() && a.eq_ignore_ascii_case(&b))
}

fn path_eq(a: char, b: char, windows: bool) -> bool {
    if windows {
        fold_eq(a, b) || (is_sep(a) && is_sep(b))
    } else {
        a == b
    }
}

fn starts_with_at(text: &[char], i: usize, pat: &[char], eq: impl Fn(char, char) -> bool) -> bool {
    i + pat.len() <= text.len() && pat.iter().enumerate().all(|(k, &p)| eq(text[i + k], p))
}

/// Whether an absolute path starts at `c[i]` (ignoring the boundary rule).
fn abs_start(c: &[char], i: usize) -> bool {
    let at = |k: usize| c.get(i + k).copied();
    let drive = matches!((at(0), at(1), at(2)), (Some(d), Some(':'), Some(s)) if d.is_ascii_alphabetic() && is_sep(s));
    let rooted = matches!((at(0), at(1)), (Some(s), Some(n)) if is_sep(s) && !is_term(n));
    drive || rooted
}

/// A drive path (`C:\`, `C:/`) or a path starting with `/` or `\`.
fn is_absolute(c: &[char]) -> bool {
    (c.len() >= 3 && c[0].is_ascii_alphabetic() && c[1] == ':' && is_sep(c[2])) || c.first().is_some_and(|&x| is_sep(x))
}

fn is_windows_path(c: &[char]) -> bool {
    (c.len() >= 2 && c[0].is_ascii_alphabetic() && c[1] == ':') || c.first() == Some(&'\\')
}

fn strip_trailing_seps(mut c: Vec<char>) -> Vec<char> {
    while c.len() > 1 && c.last().is_some_and(|&x| is_sep(x)) {
        c.pop();
    }
    c
}

/// The non-empty components of a path after its root (`C:`, `/`, `\\server\share`).
fn components_after_root(c: &[char]) -> Vec<String> {
    let s: String = c.iter().collect();
    let parts: Vec<&str> = s.split(['/', '\\']).filter(|p| !p.is_empty()).collect();
    let skip = if c.len() >= 2 && c[0].is_ascii_alphabetic() && c[1] == ':' {
        1
    } else if c.len() >= 2 && is_sep(c[0]) && is_sep(c[1]) {
        2
    } else {
        0
    };
    parts.into_iter().skip(skip).map(str::to_string).collect()
}

// ------------------------------------------------------------------ redactor

enum Seg {
    Text(Vec<char>),
    Mark(&'static str),
}

/// Redacts identifying strings from report text; built from an [`Identity`] and the install
/// directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Redactor {
    install: Vec<char>,
    install_windows: bool,
    /// Empty when the home directory is a root (nothing identifying to replace).
    home: Vec<char>,
    names: Vec<(Vec<char>, &'static str)>,
}

impl Redactor {
    /// Checks the identity (user, host and home non-empty, home absolute) and the install
    /// directory (absolute, not a root, not the home directory or one of its ancestors).
    pub fn new(identity: &Identity, install_dir: &str) -> Result<Self, ReportError> {
        let user = identity.user.trim();
        if user.is_empty() {
            return Err(ReportError::IdentityUnavailable { item: "user" });
        }
        let host = identity.host.trim();
        if host.is_empty() {
            return Err(ReportError::IdentityUnavailable { item: "host" });
        }
        let home: Vec<char> = identity.home.trim().chars().collect();
        if !is_absolute(&home) {
            return Err(ReportError::IdentityUnavailable { item: "home" });
        }
        let mut home = strip_trailing_seps(home);
        if components_after_root(&home).is_empty() {
            home.clear();
        }
        let install: Vec<char> = install_dir.trim().chars().collect();
        if !is_absolute(&install) {
            return Err(ReportError::InvalidInstallDir { detail: "not an absolute path".to_string() });
        }
        let install = strip_trailing_seps(install);
        let comps = components_after_root(&install);
        if comps.is_empty() {
            return Err(ReportError::InvalidInstallDir { detail: "is a root directory".to_string() });
        }
        if comps.iter().any(|p| p == "..") {
            return Err(ReportError::InvalidInstallDir { detail: "has a `..` component".to_string() });
        }
        let install_windows = is_windows_path(&install);
        // The install directory must not hold the home directory (it would keep every path
        // under home, and other users' homes beside it).
        if !home.is_empty()
            && starts_with_at(&home, 0, &install, |a, b| path_eq(a, b, true))
            && (home.len() == install.len() || home.get(install.len()).is_some_and(|&c| is_sep(c)))
        {
            return Err(ReportError::InvalidInstallDir { detail: "contains the home directory".to_string() });
        }
        let mut names: Vec<(Vec<char>, &'static str)> = Vec::new();
        let mut push = |s: &str, mark: &'static str| {
            let v: Vec<char> = s.chars().collect();
            let dup = names.iter().any(|(n, _)| n.len() == v.len() && n.iter().zip(&v).all(|(&a, &b)| fold_eq(a, b)));
            if !v.is_empty() && !dup {
                names.push((v, mark));
            }
        };
        push(host, super::PLACEHOLDER_HOST);
        if let Some((label, _)) = host.split_once('.') {
            push(label, super::PLACEHOLDER_HOST);
        }
        push(user, super::PLACEHOLDER_USER);
        // Longest first; ties keep the order above (host, host label, user).
        names.sort_by_key(|n| std::cmp::Reverse(n.0.len()));
        Ok(Redactor { install, install_windows, home, names })
    }

    /// The redacted form of `s` (see the module documentation for the rules).
    pub fn redact_text(&self, s: &str) -> String {
        let c: Vec<char> = s.chars().collect();
        let mut segs = self.path_pass(&c);
        segs = self.split_pass(segs, |text, i| self.home_match(text, i).map(|n| (n, super::PLACEHOLDER_HOME)));
        for (name, mark) in &self.names {
            segs = self.split_pass(segs, |text, i| word_match(text, i, name).map(|n| (n, *mark)));
        }
        let mut out = String::with_capacity(s.len());
        for seg in segs {
            match seg {
                Seg::Text(t) => out.extend(t),
                Seg::Mark(m) => out.push_str(m),
            }
        }
        out
    }

    fn path_pass(&self, c: &[char]) -> Vec<Seg> {
        let mut segs = Vec::new();
        let mut buf = Vec::new();
        let mut i = 0;
        while i < c.len() {
            let boundary = i == 0 || !(is_path_char(c[i - 1]) || is_sep(c[i - 1]));
            if boundary && abs_start(c, i) {
                let end = self.path_end(c, i);
                let tok = &c[i..end];
                if self.keeps(tok) {
                    buf.extend_from_slice(tok);
                } else {
                    if !buf.is_empty() {
                        segs.push(Seg::Text(std::mem::take(&mut buf)));
                    }
                    segs.push(Seg::Mark(super::PLACEHOLDER_PATH));
                }
                i = end;
                continue;
            }
            buf.push(c[i]);
            i += 1;
        }
        if !buf.is_empty() {
            segs.push(Seg::Text(buf));
        }
        segs
    }

    fn path_end(&self, c: &[char], i: usize) -> usize {
        let scan = |mut j: usize| {
            while j < c.len() && !is_term(c[j]) {
                j += 1;
            }
            j
        };
        let mut end = scan(i);
        let mut min_end = i + 1;
        let prefixes: [(&[char], bool); 2] = [(&self.install, self.install_windows), (&self.home, true)];
        for (p, windows) in prefixes {
            let after = i + p.len();
            if !p.is_empty()
                && starts_with_at(c, i, p, |a, b| path_eq(a, b, windows))
                && (after == c.len() || is_sep(c[after]) || is_term(c[after]))
            {
                end = end.max(scan(after));
                min_end = min_end.max(after);
            }
        }
        while end > min_end && matches!(c[end - 1], '.' | ',' | ';' | ':' | '!' | '?' | ')' | ']' | '}') {
            end -= 1;
        }
        end
    }

    fn keeps(&self, tok: &[char]) -> bool {
        let n = self.install.len();
        starts_with_at(tok, 0, &self.install, |a, b| path_eq(a, b, self.install_windows))
            && (tok.len() == n || is_sep(tok[n]))
            && !components_after_root(tok).iter().any(|p| p == "..")
    }

    fn home_match(&self, text: &[char], i: usize) -> Option<usize> {
        let n = self.home.len();
        let after_ok = !text.get(i + n).is_some_and(|&c| is_path_char(c));
        (n > 0 && starts_with_at(text, i, &self.home, |a, b| path_eq(a, b, true)) && after_ok).then_some(n)
    }

    fn split_pass(&self, segs: Vec<Seg>, find: impl Fn(&[char], usize) -> Option<(usize, &'static str)>) -> Vec<Seg> {
        let mut out = Vec::with_capacity(segs.len());
        for seg in segs {
            let text = match seg {
                Seg::Text(t) => t,
                m @ Seg::Mark(_) => {
                    out.push(m);
                    continue;
                }
            };
            let mut buf = Vec::new();
            let mut i = 0;
            while i < text.len() {
                if let Some((n, mark)) = find(&text, i) {
                    if !buf.is_empty() {
                        out.push(Seg::Text(std::mem::take(&mut buf)));
                    }
                    out.push(Seg::Mark(mark));
                    i += n;
                } else {
                    buf.push(text[i]);
                    i += 1;
                }
            }
            if !buf.is_empty() {
                out.push(Seg::Text(buf));
            }
        }
        out
    }
}

fn word_match(text: &[char], i: usize, name: &[char]) -> Option<usize> {
    let before_ok = i == 0 || !text[i - 1].is_alphanumeric();
    let after_ok = !text.get(i + name.len()).is_some_and(|c| c.is_alphanumeric());
    (!name.is_empty() && before_ok && after_ok && starts_with_at(text, i, name, fold_eq)).then_some(name.len())
}

fn redact_probed<T>(
    p: &mut Probed<T>,
    r: &Redactor,
    f: impl FnOnce(&mut T, &Redactor) -> Result<(), ReportError>,
) -> Result<(), ReportError> {
    match p {
        Probed::Known(v) => f(v, r),
        Probed::Unknown { detail, .. } => {
            *detail = fit_detail(r.redact_text(detail));
            Ok(())
        }
    }
}

/// Redacts the detail of an unknown item (the assessment's tier).
pub(super) fn redact_probed_detail<T>(p: &mut Probed<T>, r: &Redactor) {
    if let Probed::Unknown { detail, .. } = p {
        *detail = fit_detail(r.redact_text(detail));
    }
}

fn redact_opt(s: &mut Option<String>, r: &Redactor) {
    if let Some(v) = s {
        *v = r.redact_text(v);
    }
}

/// Redacts every probed free-text string of `snap` in place.
pub(super) fn redact_snapshot(snap: &mut EnvSnapshot, r: &Redactor) -> Result<(), ReportError> {
    redact_probed(&mut snap.os, r, |os, r| {
        os.family = r.redact_text(&os.family);
        os.version = r.redact_text(&os.version);
        Ok(())
    })?;
    redact_probed(&mut snap.arch, r, |arch, r| {
        let red = r.redact_text(arch.as_str());
        *arch =
            Arch::parse(&red).ok_or(ReportError::RedactionOverflow { pointer: "/snapshot/arch/value".to_string() })?;
        Ok(())
    })?;
    redact_probed(&mut snap.cpu, r, |cpu, r| {
        cpu.model = r.redact_text(&cpu.model);
        cpu.instruction_set_level = r.redact_text(&cpu.instruction_set_level);
        Ok(())
    })?;
    redact_probed(&mut snap.ram, r, |_, _| Ok(()))?;
    redact_probed(&mut snap.storage, r, |_, _| Ok(()))?;
    redact_probed(&mut snap.display, r, |_, _| Ok(()))?;
    redact_probed(&mut snap.gpu, r, |g, r| {
        g.backend = r.redact_text(&g.backend);
        redact_opt(&mut g.adapter, r);
        redact_opt(&mut g.driver_version, r);
        redact_opt(&mut g.device_type, r);
        g.fallback_cause = r.redact_text(&g.fallback_cause);
        g.reason = r.redact_text(&g.reason);
        Ok(())
    })?;
    Ok(())
}
