//! `cargo xtask rel version-check` and `cargo xtask rel version-compare`: the product version
//! scheme and tag check (spec node `ECO.REL.PROC.VERSION`).
//!
//! A release tag is `v<MAJOR>.<MINOR>.<PATCH>` with an optional `-beta.<N>` (N >= 1) or
//! `-nightly.<YYYYMMDD>` suffix and no leading zeros. The check is offline: it reads the tag
//! from its argument (never from the network, never creating or moving tags), the product
//! version from `[workspace.package] version` of the workspace `Cargo.toml`, and the version
//! each built binary reports with `--version --json`. SDK crates carry their own versions and
//! are not read.
//!
//! The parser is hand written (no regex) and refuses input over [`MAX_VERSION_BYTES`].
//! Precedence follows Semantic Versioning 2.0.0: a pre-release is lower than its release, and
//! pre-release identifiers compare left to right, numeric ones numerically and alphanumeric
//! ones in ASCII order, so every `beta` is lower than every `nightly` of the same core version.
//! Everything except running binaries is a pure function of its input (D0).

use fid_canon::{parse_with_limits, to_canonical_bytes, CValue};
use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::fmt;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Longest accepted tag or version string, in bytes.
pub const MAX_VERSION_BYTES: usize = 64;
/// Largest workspace `Cargo.toml` read.
pub const MAX_MANIFEST_BYTES: u64 = 1 << 20;
/// Largest `--version --json` output read from a binary.
pub const MAX_OUTPUT_BYTES: usize = 64 * 1024;
/// How long a binary may take to answer `--version --json`.
pub const BINARY_TIMEOUT: Duration = Duration::from_secs(10);

/// A calendar date from a `-nightly.YYYYMMDD` suffix.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Date {
    pub year: u16,
    pub month: u8,
    pub day: u8,
}

impl Date {
    /// The date as the number `YYYYMMDD` (its SemVer numeric identifier).
    pub fn number(self) -> u64 {
        u64::from(self.year) * 10_000 + u64::from(self.month) * 100 + u64::from(self.day)
    }
}

/// A pre-release suffix.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pre {
    Beta(u32),
    Nightly(Date),
}

impl Pre {
    /// The two SemVer pre-release identifiers: a label and a number.
    fn identifiers(self) -> (&'static str, u64) {
        match self {
            Pre::Beta(n) => ("beta", u64::from(n)),
            Pre::Nightly(d) => ("nightly", d.number()),
        }
    }
}

/// A product version.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Version {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
    pub pre: Option<Pre>,
}

impl Version {
    /// Semantic Versioning 2.0.0 precedence.
    pub fn precedence(&self, other: &Version) -> Ordering {
        (self.major, self.minor, self.patch).cmp(&(other.major, other.minor, other.patch)).then_with(|| {
            match (self.pre, other.pre) {
                (None, None) => Ordering::Equal,
                (None, Some(_)) => Ordering::Greater,
                (Some(_), None) => Ordering::Less,
                (Some(a), Some(b)) => {
                    let ((la, na), (lb, nb)) = (a.identifiers(), b.identifiers());
                    // Alphanumeric identifiers compare in ASCII order; numeric ones numerically.
                    la.as_bytes().cmp(lb.as_bytes()).then(na.cmp(&nb))
                }
            }
        })
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)?;
        match self.pre {
            None => Ok(()),
            Some(Pre::Beta(n)) => write!(f, "-beta.{n}"),
            Some(Pre::Nightly(d)) => write!(f, "-nightly.{:04}{:02}{:02}", d.year, d.month, d.day),
        }
    }
}

/// A release channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Channel {
    Nightly,
    Beta,
    Stable,
    Lts,
}

impl Channel {
    pub const ALL: [Channel; 4] = [Channel::Nightly, Channel::Beta, Channel::Stable, Channel::Lts];

    pub fn name(self) -> &'static str {
        match self {
            Channel::Nightly => "nightly",
            Channel::Beta => "beta",
            Channel::Stable => "stable",
            Channel::Lts => "lts",
        }
    }

    pub fn parse(s: &str) -> Option<Channel> {
        Channel::ALL.into_iter().find(|c| c.name() == s)
    }

    /// Whether a version's suffix is the one this channel requires.
    pub fn allows(self, v: &Version) -> bool {
        matches!(
            (self, v.pre),
            (Channel::Nightly, Some(Pre::Nightly(_)))
                | (Channel::Beta, Some(Pre::Beta(_)))
                | (Channel::Stable | Channel::Lts, None)
        )
    }
}

/// A failed check. Codes are stable: see [`VersionError::code`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VersionError {
    /// The tag (or version) does not have the required form.
    TagFormat { input: String },
    /// The tag's suffix is not the one the channel requires.
    ChannelSuffix { tag: String, channel: Channel },
    /// The `-nightly.YYYYMMDD` date is not a calendar date (or has a year below 1000).
    Date { input: String },
    /// The workspace or a binary reports another version than the tag.
    Mismatch { binary: String, found: String, expected: String },
    /// A binary could not be run or gave no readable version.
    BinaryFailed { binary: String, reason: String },
}

impl VersionError {
    pub fn code(&self) -> &'static str {
        match self {
            VersionError::TagFormat { .. } => "VERSION-TAG-FORMAT",
            VersionError::ChannelSuffix { .. } => "VERSION-CHANNEL-SUFFIX",
            VersionError::Date { .. } => "VERSION-DATE",
            VersionError::Mismatch { .. } => "VERSION-MISMATCH",
            VersionError::BinaryFailed { .. } => "VERSION-BINARY-FAILED",
        }
    }

    /// The binary (or `workspace`) a mismatch or failure is about.
    pub fn subject(&self) -> Option<&str> {
        match self {
            VersionError::Mismatch { binary, .. } | VersionError::BinaryFailed { binary, .. } => Some(binary),
            _ => None,
        }
    }

    fn detail(&self) -> String {
        match self {
            VersionError::TagFormat { input } => {
                format!("{input:?} is not v<MAJOR>.<MINOR>.<PATCH>[-beta.<N>|-nightly.<YYYYMMDD>]")
            }
            VersionError::ChannelSuffix { tag, channel } => {
                let need = match channel {
                    Channel::Nightly => "a -nightly.<YYYYMMDD> suffix",
                    Channel::Beta => "a -beta.<N> suffix",
                    Channel::Stable | Channel::Lts => "no suffix",
                };
                format!("{tag:?} on channel {} needs {need}", channel.name())
            }
            VersionError::Date { input } => format!("{input:?} has no valid nightly date"),
            VersionError::Mismatch { found, expected, .. } => format!("reports {found:?}, tag version is {expected}"),
            VersionError::BinaryFailed { reason, .. } => reason.clone(),
        }
    }
}

impl fmt::Display for VersionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.subject() {
            Some(s) => write!(f, "{}({s}): {}", self.code(), self.detail()),
            None => write!(f, "{}: {}", self.code(), self.detail()),
        }
    }
}

/// Shortens untrusted text for messages.
fn clip(s: &str) -> String {
    if s.len() <= MAX_VERSION_BYTES {
        return s.to_string();
    }
    let mut end = MAX_VERSION_BYTES;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}...", &s[..end])
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ParseError {
    Format,
    Date,
}

/// A numeric identifier: ASCII digits, no leading zero, fits in u64.
fn number(s: &str) -> Result<u64, ParseError> {
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) || (s.len() > 1 && s.starts_with('0')) {
        return Err(ParseError::Format);
    }
    s.parse::<u64>().map_err(|_| ParseError::Format)
}

fn is_leap(year: u16) -> bool {
    year % 4 == 0 && (year % 100 != 0 || year % 400 == 0)
}

fn days_in_month(year: u16, month: u8) -> u8 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap(year) => 29,
        2 => 28,
        _ => 0,
    }
}

/// Eight ASCII digits as a Gregorian date. Years below 1000 are refused: their `YYYYMMDD`
/// starts with `0`, which SemVer forbids in a numeric identifier.
fn date(s: &str) -> Result<Date, ParseError> {
    if s.len() != 8 || !s.bytes().all(|b| b.is_ascii_digit()) {
        return Err(ParseError::Format);
    }
    let field = |r: std::ops::Range<usize>| s.get(r).and_then(|x| x.parse::<u16>().ok()).ok_or(ParseError::Format);
    let (year, month, day) = (field(0..4)?, field(4..6)?, field(6..8)?);
    let (Ok(month), Ok(day)) = (u8::try_from(month), u8::try_from(day)) else {
        return Err(ParseError::Date);
    };
    if year < 1000 || day == 0 || day > days_in_month(year, month) {
        return Err(ParseError::Date);
    }
    Ok(Date { year, month, day })
}

fn parse_inner(s: &str) -> Result<Version, ParseError> {
    if s.len() > MAX_VERSION_BYTES {
        return Err(ParseError::Format);
    }
    let (core, pre) = match s.split_once('-') {
        Some((c, p)) => (c, Some(p)),
        None => (s, None),
    };
    let mut parts = core.split('.');
    let mut next = || number(parts.next().unwrap_or(""));
    let (major, minor, patch) = (next()?, next()?, next()?);
    if parts.next().is_some() {
        return Err(ParseError::Format);
    }
    let pre = match pre {
        None => None,
        Some(p) => {
            if let Some(n) = p.strip_prefix("beta.") {
                let n = number(n)?;
                if n == 0 {
                    return Err(ParseError::Format);
                }
                Some(Pre::Beta(u32::try_from(n).map_err(|_| ParseError::Format)?))
            } else if let Some(d) = p.strip_prefix("nightly.") {
                Some(Pre::Nightly(date(d)?))
            } else {
                return Err(ParseError::Format);
            }
        }
    };
    Ok(Version { major, minor, patch, pre })
}

fn to_error(e: ParseError, input: &str) -> VersionError {
    match e {
        ParseError::Format => VersionError::TagFormat { input: clip(input) },
        ParseError::Date => VersionError::Date { input: clip(input) },
    }
}

/// Parses a version without the `v` (as in `Cargo.toml` or `--version --json` output).
pub fn parse_version(s: &str) -> Result<Version, VersionError> {
    parse_inner(s).map_err(|e| to_error(e, s))
}

/// Parses a release tag (`v` + version). Errors are `VERSION-TAG-FORMAT` or `VERSION-DATE`.
pub fn parse_tag(tag: &str) -> Result<Version, VersionError> {
    let inner = if tag.len() > MAX_VERSION_BYTES + 1 { Err(ParseError::Format) } else { Ok(tag) };
    inner
        .and_then(|t| t.strip_prefix('v').ok_or(ParseError::Format))
        .and_then(parse_inner)
        .map_err(|e| to_error(e, tag))
}

/// Parses either form, for `version-compare`.
pub fn parse_any(s: &str) -> Result<Version, VersionError> {
    if s.starts_with('v') {
        parse_tag(s)
    } else {
        parse_version(s)
    }
}

/// Checks the tag's form, date and channel suffix, in that order.
pub fn check_tag(tag: &str, channel: Channel) -> Result<Version, VersionError> {
    let v = parse_tag(tag)?;
    if !channel.allows(&v) {
        return Err(VersionError::ChannelSuffix { tag: clip(tag), channel });
    }
    Ok(v)
}

/// Reads `[workspace.package] version` from `<dir>/Cargo.toml`.
pub fn workspace_version(dir: &Path) -> Result<String, String> {
    let path = dir.join("Cargo.toml");
    let mut bytes = Vec::new();
    std::fs::File::open(&path)
        .and_then(|f| f.take(MAX_MANIFEST_BYTES + 1).read_to_end(&mut bytes))
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    if bytes.len() as u64 > MAX_MANIFEST_BYTES {
        return Err(format!("{} is larger than {MAX_MANIFEST_BYTES} bytes", path.display()));
    }
    let text = String::from_utf8(bytes).map_err(|_| format!("{} is not UTF-8", path.display()))?;
    let table = text.parse::<toml::Table>().map_err(|e| format!("{}: {}", path.display(), e.message()))?;
    table
        .get("workspace")
        .and_then(|w| w.get("package"))
        .and_then(|p| p.get("version"))
        .and_then(|v| v.as_str())
        .map(String::from)
        .ok_or_else(|| format!("{} has no [workspace.package] version", path.display()))
}

/// The binaries in `dir`, as (name, path) sorted by name, then file name. On Windows these are files
/// ending in `.exe`, `.cmd` or `.bat` (the name drops the extension); elsewhere, files whose
/// name has no `.` and does not start with one.
pub fn list_binaries(dir: &Path) -> Result<Vec<(String, PathBuf)>, String> {
    let entries = std::fs::read_dir(dir).map_err(|e| format!("cannot list {}: {e}", dir.display()))?;
    let mut found = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|e| format!("cannot list {}: {e}", dir.display()))?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Some(file) = path.file_name().and_then(|n| n.to_str()).map(String::from) else { continue };
        let name = if cfg!(windows) {
            let lower = file.to_ascii_lowercase();
            [".exe", ".cmd", ".bat"]
                .iter()
                .find(|ext| lower.ends_with(*ext))
                .and_then(|ext| file.get(..file.len() - ext.len()))
                .map(String::from)
        } else if file.contains('.') {
            None
        } else {
            Some(file.clone())
        };
        if let Some(name) = name.filter(|n| !n.is_empty()) {
            found.push((name, file, path));
        }
    }
    found.sort();
    Ok(found.into_iter().map(|(name, _, path)| (name, path)).collect())
}

/// Runs `<path> --version --json` and returns the `version` string it prints. Errors are
/// the reason for `VERSION-BINARY-FAILED`.
pub fn binary_version(path: &Path, timeout: Duration) -> Result<String, String> {
    let mut child = Command::new(path)
        .args(["--version", "--json"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("cannot start: {e}"))?;
    let Some(stdout) = child.stdout.take() else {
        let _ = child.kill();
        let _ = child.wait();
        return Err("no stdout".into());
    };
    let reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        stdout.take(MAX_OUTPUT_BYTES as u64 + 1).read_to_end(&mut buf).map(|_| buf)
    });
    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break s,
            Ok(None) if start.elapsed() >= timeout => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("no answer within {} s", timeout.as_secs()));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(10)),
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("cannot wait: {e}"));
            }
        }
    };
    if !status.success() {
        return Err(format!("--version --json failed ({status})"));
    }
    let buf = match reader.join() {
        Ok(Ok(b)) => b,
        Ok(Err(e)) => return Err(format!("cannot read output: {e}")),
        Err(_) => return Err("cannot read output".into()),
    };
    if buf.len() > MAX_OUTPUT_BYTES {
        return Err(format!("output is larger than {MAX_OUTPUT_BYTES} bytes"));
    }
    let text = buf.trim_ascii();
    match parse_with_limits(text, MAX_OUTPUT_BYTES, 16) {
        Ok(CValue::Obj(o)) => match o.get("version") {
            Some(CValue::Str(v)) => Ok(v.clone()),
            _ => Err("output has no \"version\" string".into()),
        },
        Ok(_) => Err("output is not a JSON object".into()),
        Err(e) => Err(format!("output is not JSON: {e}")),
    }
}

/// Compares a reported version string with the tag version.
fn compare_reported(binary: &str, found: &str, expected: &Version) -> Option<VersionError> {
    match parse_version(found) {
        Ok(v) if v == *expected => None,
        _ => Some(VersionError::Mismatch {
            binary: binary.to_string(),
            found: clip(found),
            expected: expected.to_string(),
        }),
    }
}

/// The full check: tag form, date and channel; then the workspace product version; then every
/// binary in `binaries` (when given). Returns the tag version, or every finding in a fixed
/// order (tag findings stop the check; workspace before binaries; binaries by name).
pub fn version_check(
    tag: &str,
    channel: Channel,
    workspace: &Path,
    binaries: Option<&Path>,
) -> Result<Version, Vec<VersionError>> {
    let expected = check_tag(tag, channel).map_err(|e| vec![e])?;
    let mut findings = Vec::new();
    match workspace_version(workspace) {
        Ok(found) => findings.extend(compare_reported("workspace", &found, &expected)),
        Err(reason) => findings.push(VersionError::Mismatch {
            binary: "workspace".into(),
            found: reason,
            expected: expected.to_string(),
        }),
    }
    if let Some(dir) = binaries {
        match list_binaries(dir) {
            Ok(list) => {
                for (name, path) in list {
                    match binary_version(&path, BINARY_TIMEOUT) {
                        Ok(found) => findings.extend(compare_reported(&name, &found, &expected)),
                        Err(reason) => findings.push(VersionError::BinaryFailed { binary: name, reason }),
                    }
                }
            }
            Err(reason) => findings.push(VersionError::BinaryFailed { binary: dir.display().to_string(), reason }),
        }
    }
    if findings.is_empty() {
        Ok(expected)
    } else {
        Err(findings)
    }
}

/// The `--json` report: `{"channel","findings":[{"code","message","subject"}],"ok","tag","version"}`
/// in canonical JSON (RFC 8785), so equal inputs give equal bytes.
pub fn report_json(tag: &str, channel: Channel, result: &Result<Version, Vec<VersionError>>) -> String {
    let s = |x: &str| CValue::Str(x.to_string());
    let mut o = BTreeMap::new();
    o.insert("tag".to_string(), s(&clip(tag)));
    o.insert("channel".to_string(), s(channel.name()));
    o.insert("ok".to_string(), CValue::Bool(result.is_ok()));
    let (version, findings) = match result {
        Ok(v) => (s(&v.to_string()), Vec::new()),
        Err(fs) => (
            CValue::Null,
            fs.iter()
                .map(|f| {
                    let mut m = BTreeMap::new();
                    m.insert("code".to_string(), s(f.code()));
                    m.insert("subject".to_string(), f.subject().map_or(CValue::Null, s));
                    m.insert("message".to_string(), s(&f.to_string()));
                    CValue::Obj(m)
                })
                .collect(),
        ),
    };
    o.insert("version".to_string(), version);
    o.insert("findings".to_string(), CValue::Arr(findings));
    to_canonical_bytes(&CValue::Obj(o))
        .ok()
        .and_then(|b| String::from_utf8(b).ok())
        .unwrap_or_else(|| "{\"ok\":false}".to_string())
}

/// `lt`, `eq` or `gt`.
pub fn ordering_word(o: Ordering) -> &'static str {
    match o {
        Ordering::Less => "lt",
        Ordering::Equal => "eq",
        Ordering::Greater => "gt",
    }
}
