//! Redaction of identifying strings (REQ-PLAT.DOCTOR.REPORT-003).
//!
//! [`Redactor::redact_text`] applies three passes to a string, in this order; text already
//! replaced by a placeholder is never scanned again:
//!
//! 1. **Paths.** A path starts where the previous character is absent or is not
//!    alphanumeric and not one of `. _ - ~ / \`, and is
//!    - a drive path (`C:\`, `C:/`), or a `/` or `\` followed by a character that does not
//!      end a path (so `\\server\share`, `\\?\C:\...`, rooted `\dir` and `file:///...`'s
//!      `///...` count too); or
//!    - `~` followed by `/` or `\`: the `~` becomes [`PLACEHOLDER_HOME`] and the rest is
//!      redacted as a path.
//!
//!    A path runs until whitespace, a control or format character, or one of
//!    ``" ' ` < > |``; when the install directory or the home directory starts there (after
//!    a `\\?\` prefix, if any), it runs at least to that directory's end, so a directory name
//!    with spaces is taken whole. Trailing `. , ; : ! ? ) ] }` are left outside it. A path
//!    under the install directory (component-wise prefix; case-insensitive and `/` = `\`
//!    only for a Windows-style install directory) with no `..` component is kept; every
//!    other path becomes [`PLACEHOLDER_PATH`]. A redacted path then swallows more text, so
//!    the rest of a path with spaces cannot leak: when the path opens a quoted string
//!    (`"`, `'` or `` ` `` just before it), everything up to the closing quote or the end of
//!    the line; otherwise every following whitespace-separated token on the same line that
//!    holds a `/` or `\`, until a token without one or a token that starts a path itself.
//! 2. **Home.** Every occurrence of the home directory (case-insensitive, `/` = `\`) that is
//!    not followed by a path character (alphanumeric, `. _ - ~`) becomes
//!    [`PLACEHOLDER_HOME`]. After pass 1 this only happens inside kept install paths and in
//!    text that is not a path.
//! 3. **Names.** Every occurrence, longest first, of each host name and its first DNS label,
//!    each logon domain and its first label ([`PLACEHOLDER_HOST`]), the user name and the
//!    home directory's last component ([`PLACEHOLDER_USER`]) that matches case-insensitively
//!    and has no alphanumeric character on either side. So `CORP\jdoe` and
//!    `jdoe@corp.example.com` lose both parts, and a renamed profile folder name is caught.
//!
//! "Case-insensitive" is Unicode simple case folding as approximated by [`fold_char`]; every
//! comparison is per Unicode scalar value (no normalisation, so a decomposed `é` does not
//! match a precomposed one).

use super::{fit_detail, ReportError};
use crate::probe::{Arch, EnvSnapshot, Probed};
use std::ffi::OsString;

/// Replaces the current user name.
pub const PLACEHOLDER_USER: &str = "<redacted:user>";
/// Replaces a host name or a logon domain.
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
    /// Every host name known for the machine (at least one): on Windows `COMPUTERNAME`, the
    /// DNS host name and the fully qualified name; elsewhere `HOSTNAME` and the kernel's.
    pub hosts: Vec<String>,
    /// Absolute home directory.
    pub home: String,
    /// Logon domains (Windows `USERDOMAIN` and `USERDNSDOMAIN`); may be empty.
    pub domains: Vec<String>,
}

/// Which OS conventions [`Identity::from_lookup`] reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdentityPlatform {
    Windows,
    Unix,
}

fn unavailable(item: &'static str) -> ReportError {
    ReportError::IdentityUnavailable { item }
}

impl Identity {
    /// Gathers the identity from the OS at call time ([`Identity::from_lookup`] over the
    /// process environment and the OS host-name sources). Fails with
    /// [`ReportError::IdentityUnavailable`] when one cannot be determined, since redaction
    /// could not then be guaranteed.
    pub fn from_os() -> Result<Identity, ReportError> {
        let platform = if cfg!(windows) { IdentityPlatform::Windows } else { IdentityPlatform::Unix };
        Self::from_lookup(platform, |name| std::env::var_os(name), os_host_names)
    }

    /// Builds the identity from an environment lookup and the host names the OS reports
    /// (`sys_hosts`). A variable that is set but not valid UTF-8 fails (`identity_unavailable`
    /// naming its item: `user`, `host`, `home` or `domain`), as does a user, home or host that
    /// is missing or blank, or a home that is not absolute.
    ///
    /// - Windows: user `USERNAME`; home `USERPROFILE`, else `HOMEDRIVE` + `HOMEPATH`; hosts
    ///   `COMPUTERNAME`, then `sys_hosts`, then each dotless host joined with each dotted
    ///   domain; domains `USERDOMAIN`, `USERDNSDOMAIN`.
    /// - Unix: user `USER`, else `LOGNAME`; home `HOME`; hosts `HOSTNAME`, then `sys_hosts`.
    pub fn from_lookup(
        platform: IdentityPlatform,
        env: impl Fn(&str) -> Option<OsString>,
        sys_hosts: impl FnOnce() -> Vec<String>,
    ) -> Result<Identity, ReportError> {
        let get = |name: &str, item: &'static str| -> Result<Option<String>, ReportError> {
            match env(name) {
                None => Ok(None),
                Some(v) => match v.into_string() {
                    Ok(s) if s.trim().is_empty() => Ok(None),
                    Ok(s) => Ok(Some(s.trim().to_string())),
                    Err(_) => Err(unavailable(item)),
                },
            }
        };
        let (home, user, env_hosts, domains) = match platform {
            IdentityPlatform::Windows => {
                let home = match get("USERPROFILE", "home")? {
                    Some(h) => Some(h),
                    None => match (get("HOMEDRIVE", "home")?, get("HOMEPATH", "home")?) {
                        (Some(d), Some(p)) => Some(format!("{d}{p}")),
                        _ => None,
                    },
                };
                let user = get("USERNAME", "user")?;
                let hosts: Vec<String> = get("COMPUTERNAME", "host")?.into_iter().collect();
                let domains: Vec<String> =
                    [get("USERDOMAIN", "domain")?, get("USERDNSDOMAIN", "domain")?].into_iter().flatten().collect();
                (home, user, hosts, domains)
            }
            IdentityPlatform::Unix => {
                let home = get("HOME", "home")?;
                let user = match get("USER", "user")? {
                    Some(u) => Some(u),
                    None => get("LOGNAME", "user")?,
                };
                let hosts: Vec<String> = get("HOSTNAME", "host")?.into_iter().collect();
                (home, user, hosts, Vec::new())
            }
        };
        let home = home.filter(|h| is_absolute(&h.chars().collect::<Vec<_>>())).ok_or_else(|| unavailable("home"))?;
        let user = user.ok_or_else(|| unavailable("user"))?;
        let mut hosts: Vec<String> = Vec::new();
        for h in env_hosts.into_iter().chain(sys_hosts()) {
            if let Some(h) = host_ok(&h) {
                if !hosts.iter().any(|x| fold_str_eq(x, &h)) {
                    hosts.push(h);
                }
            }
        }
        if platform == IdentityPlatform::Windows {
            let fqdns: Vec<String> = hosts
                .iter()
                .filter(|h| !h.contains('.'))
                .flat_map(|h| domains.iter().filter(|d| d.contains('.')).map(move |d| format!("{h}.{d}")))
                .collect();
            for f in fqdns {
                if let Some(f) = host_ok(&f) {
                    if !hosts.iter().any(|x| fold_str_eq(x, &f)) {
                        hosts.push(f);
                    }
                }
            }
        }
        if hosts.is_empty() {
            return Err(unavailable("host"));
        }
        Ok(Identity { user, hosts, home, domains })
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

fn command_host(program: &str) -> Option<String> {
    let out = std::process::Command::new(program).output().ok()?;
    if !out.status.success() || out.stdout.len() > MAX_HOST_BYTES + 2 {
        return None;
    }
    String::from_utf8(out.stdout).ok().and_then(|s| host_ok(&s))
}

/// The host names the OS reports besides the environment.
#[cfg(windows)]
fn os_host_names() -> Vec<String> {
    // `hostname.exe` prints the DNS host name, which may be longer than the 15-character
    // NetBIOS `COMPUTERNAME`. Called by absolute path, not through `PATH`.
    let root = std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".to_string());
    command_host(&format!("{root}\\System32\\hostname.exe")).into_iter().collect()
}

/// The host names the OS reports besides the environment.
#[cfg(not(windows))]
fn os_host_names() -> Vec<String> {
    use std::io::Read;
    let read_small = |path: &str| -> Option<String> {
        let f = std::fs::File::open(path).ok()?;
        let mut buf = Vec::new();
        f.take(MAX_HOST_BYTES as u64 + 2).read_to_end(&mut buf).ok()?;
        String::from_utf8(buf).ok().and_then(|s| host_ok(&s))
    };
    let mut out: Vec<String> =
        ["/proc/sys/kernel/hostname", "/etc/hostname"].into_iter().filter_map(read_small).collect();
    out.extend(command_host("hostname"));
    out
}

// ------------------------------------------------------------------ character classes

/// Unicode simple case folding, approximated: the lowercase mapping when it is a single
/// character, plus the simple foldings that differ from lowercase (`ς` → `σ`, `ſ` → `s`,
/// U+0345 and U+1FBE → `ι`, `ϐ ϑ ϕ ϖ ϰ ϱ ϵ` → `β θ φ π κ ρ ε`, `ẛ` → `ṡ`). Characters whose
/// lowercase is longer (e.g. `İ`) fold to themselves.
pub fn fold_char(c: char) -> char {
    match c {
        'ς' => 'σ',
        'ſ' => 's',
        '\u{0345}' | '\u{1FBE}' => 'ι',
        'ϐ' => 'β',
        'ϑ' => 'θ',
        'ϕ' => 'φ',
        'ϖ' => 'π',
        'ϰ' => 'κ',
        'ϱ' => 'ρ',
        'ϵ' => 'ε',
        'ẛ' => 'ṡ',
        _ => {
            let mut l = c.to_lowercase();
            match (l.next(), l.next()) {
                (Some(x), None) => x,
                _ => c,
            }
        }
    }
}

fn fold_eq(a: char, b: char) -> bool {
    a == b || fold_char(a) == fold_char(b)
}

fn fold_str_eq(a: &str, b: &str) -> bool {
    a.chars().count() == b.chars().count() && a.chars().zip(b.chars()).all(|(x, y)| fold_eq(x, y))
}

fn is_sep(c: char) -> bool {
    c == '/' || c == '\\'
}

/// Ends a path token.
fn is_term(c: char) -> bool {
    c.is_whitespace() || super::is_escaped_char(c) || matches!(c, '"' | '\'' | '`' | '<' | '>' | '|')
}

fn is_line_break(c: char) -> bool {
    matches!(c, '\n' | '\r' | '\u{b}' | '\u{c}' | '\u{85}' | '\u{2028}' | '\u{2029}')
}

/// May continue a name or path component.
fn is_path_char(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '.' | '_' | '-' | '~')
}

fn is_trailing_punct(c: char) -> bool {
    matches!(c, '.' | ',' | ';' | ':' | '!' | '?' | ')' | ']' | '}')
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

/// Whether `~/` or `~\` starts at `c[i]`.
fn tilde_home(c: &[char], i: usize) -> bool {
    c.get(i) == Some(&'~') && c.get(i + 1).is_some_and(|&s| is_sep(s))
}

/// Length of a `\\?\` prefix at `c[i]` (0 if none).
fn verbatim_len(c: &[char], i: usize) -> usize {
    if starts_with_at(c, i, &['\\', '\\', '?', '\\'], |a, b| a == b) {
        4
    } else {
        0
    }
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
    /// Checks the identity (user, a host and home non-blank, home absolute) and the install
    /// directory (absolute, not a root, no `..`, not the home directory or one of its
    /// ancestors).
    pub fn new(identity: &Identity, install_dir: &str) -> Result<Self, ReportError> {
        let user = identity.user.trim();
        if user.is_empty() {
            return Err(unavailable("user"));
        }
        let hosts: Vec<&str> = identity.hosts.iter().map(|h| h.trim()).filter(|h| !h.is_empty()).collect();
        if hosts.is_empty() {
            return Err(unavailable("host"));
        }
        let home: Vec<char> = identity.home.trim().chars().collect();
        if !is_absolute(&home) {
            return Err(unavailable("home"));
        }
        let mut home = strip_trailing_seps(home);
        let home_last = components_after_root(&home).pop();
        if home_last.is_none() {
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
            let v: Vec<char> = s.trim().chars().collect();
            let dup = names.iter().any(|(n, _)| n.len() == v.len() && n.iter().zip(&v).all(|(&a, &b)| fold_eq(a, b)));
            if !v.is_empty() && !dup {
                names.push((v, mark));
            }
        };
        let with_label = |s: &str| -> Vec<String> {
            let mut v = vec![s.to_string()];
            if let Some((label, _)) = s.split_once('.') {
                v.push(label.to_string());
            }
            v
        };
        for h in &hosts {
            for n in with_label(h) {
                push(&n, super::PLACEHOLDER_HOST);
            }
        }
        for d in &identity.domains {
            for n in with_label(d.trim()) {
                push(&n, super::PLACEHOLDER_HOST);
            }
        }
        push(user, super::PLACEHOLDER_USER);
        if let Some(last) = &home_last {
            push(last, super::PLACEHOLDER_USER);
        }
        // Longest first; ties keep the order above.
        names.sort_by_key(|n| std::cmp::Reverse(n.0.len()));
        Ok(Redactor { install, install_windows, home, names })
    }

    /// The redacted form of `s` (see the module documentation for the rules).
    pub fn redact_text(&self, s: &str) -> String {
        let c: Vec<char> = s.chars().collect();
        let mut segs = self.path_pass(&c);
        segs = split_pass(segs, |text, i| self.home_match(text, i).map(|n| (n, super::PLACEHOLDER_HOME)));
        for (name, mark) in &self.names {
            segs = split_pass(segs, |text, i| word_match(text, i, name).map(|n| (n, *mark)));
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
        let flush = |buf: &mut Vec<char>, segs: &mut Vec<Seg>| {
            if !buf.is_empty() {
                segs.push(Seg::Text(std::mem::take(buf)));
            }
        };
        let mut i = 0;
        while i < c.len() {
            let before = i.checked_sub(1).map(|k| c[k]);
            let boundary = before.map_or(true, |p| !(is_path_char(p) || is_sep(p)));
            if boundary && tilde_home(c, i) {
                flush(&mut buf, &mut segs);
                segs.push(Seg::Mark(super::PLACEHOLDER_HOME));
                let end = redacted_end(c, self.path_end(c, i + 1), before);
                segs.push(Seg::Mark(super::PLACEHOLDER_PATH));
                i = end;
                continue;
            }
            if boundary && abs_start(c, i) {
                let end = self.path_end(c, i);
                if self.keeps(&c[i..end]) {
                    buf.extend_from_slice(&c[i..end]);
                    i = end;
                } else {
                    flush(&mut buf, &mut segs);
                    segs.push(Seg::Mark(super::PLACEHOLDER_PATH));
                    i = redacted_end(c, end, before);
                }
                continue;
            }
            buf.push(c[i]);
            i += 1;
        }
        flush(&mut buf, &mut segs);
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
        let base = i + verbatim_len(c, i);
        let prefixes: [(&[char], bool); 2] = [(&self.install, self.install_windows), (&self.home, true)];
        for (p, windows) in prefixes {
            let after = base + p.len();
            if !p.is_empty()
                && starts_with_at(c, base, p, |a, b| path_eq(a, b, windows))
                && (after == c.len() || is_sep(c[after]) || is_term(c[after]))
            {
                end = end.max(scan(after));
                min_end = min_end.max(after);
            }
        }
        while end > min_end && is_trailing_punct(c[end - 1]) {
            end -= 1;
        }
        end
    }

    fn keeps(&self, tok: &[char]) -> bool {
        let tok = &tok[verbatim_len(tok, 0)..];
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
}

/// Where a redacted path that ends at `end` stops swallowing text (pass 1).
fn redacted_end(c: &[char], end: usize, before: Option<char>) -> usize {
    if let Some(q) = before.filter(|q| matches!(q, '"' | '\'' | '`')) {
        let mut j = end;
        while j < c.len() && c[j] != q && !is_line_break(c[j]) {
            j += 1;
        }
        return j;
    }
    let mut end = end;
    loop {
        let mut j = end;
        while j < c.len() && c[j].is_whitespace() && !is_line_break(c[j]) {
            j += 1;
        }
        if j == end || j >= c.len() || abs_start(c, j) || tilde_home(c, j) {
            return end;
        }
        let mut k = j;
        while k < c.len() && !is_term(c[k]) {
            k += 1;
        }
        if !c[j..k].iter().any(|&x| is_sep(x)) {
            return end;
        }
        while k > j + 1 && is_trailing_punct(c[k - 1]) {
            k -= 1;
        }
        end = k;
    }
}

fn split_pass(segs: Vec<Seg>, find: impl Fn(&[char], usize) -> Option<(usize, &'static str)>) -> Vec<Seg> {
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
