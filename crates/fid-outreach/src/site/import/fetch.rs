//! `fid-outreach site import fetch`: adds one release's manifest and reports to the site's
//! vendored inputs (spec node `OUTR.SITE.GEN.IMPORT`, REQ-006 and REQ-007).
//!
//! The host of `--base-url` must be listed in `<site>/hosts.toml` (`[[host]] name, purpose`;
//! compared case-insensitively and exactly, no wildcard), checked before any connection.
//! `https://` is used for every host; plain `http://` only for loopback hosts (`127.0.0.1`,
//! `::1`, `localhost`), which the tests use. Downloads go through the `curl` program, as in
//! `fid-io-corp`, so the crate links no TLS stack; redirects are not followed.
//!
//! All files are downloaded into `<site>/.inputs-fetch.tmp/` and checked against the release
//! manifest first; only when every check passes are they moved into `inputs/<tag>/` and the
//! lock file rewritten (sorted by path, through a temporary file). On any failure the temporary
//! folder is removed, nothing is written under `inputs/` and the lock file is left unchanged.

use super::{
    hash_bytes, is_tag, parse_err, parse_lock, parse_release_manifest, read_bounded, render_lock, tag_matches, Finding,
    ImportError, LockEntry, HOSTS_FILE, LOCK_FILE, MAX_HOSTS_BYTES, MAX_LOCK_BYTES, MAX_MANIFEST_BYTES,
    MAX_REPORT_BYTES, RELEASE_MANIFEST,
};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Report files fetched when the release manifest lists them.
pub const REPORTS: [&str; 4] = ["parity.json", "parity.md", "status.json", "status.md"];
/// Host purposes `hosts.toml` accepts.
pub const PURPOSES: [&str; 3] = ["release_host", "mirror", "site"];
/// Scratch folder inside the site folder (outside `inputs/`).
pub const SCRATCH: &str = ".inputs-fetch.tmp";

const LOOPBACK: [&str; 3] = ["127.0.0.1", "::1", "localhost"];

/// Arguments of [`fetch`].
#[derive(Debug, Clone, Copy)]
pub struct FetchArgs<'a> {
    pub site: &'a Path,
    pub release: &'a str,
    pub base_url: &'a str,
}

/// A parsed `--base-url`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BaseUrl {
    pub https: bool,
    /// Lowercase host, without brackets for IPv6.
    pub host: String,
    /// The URL with a trailing `/`, ready to have a file name appended.
    pub prefix: String,
}

/// Parses `http://` or `https://` URLs with a host, an optional port and a path; user info,
/// queries, fragments, whitespace and curl glob characters are refused.
pub fn parse_base_url(url: &str) -> Option<BaseUrl> {
    let (https, rest) =
        if let Some(r) = url.strip_prefix("https://") { (true, r) } else { (false, url.strip_prefix("http://")?) };
    if url.len() > 2048
        || url.chars().any(|c| c.is_whitespace() || c.is_control() || matches!(c, '{' | '}' | '?' | '#' | '@' | '\\'))
    {
        return None;
    }
    let (authority, path) = rest.split_once('/').unwrap_or((rest, ""));
    if path.contains(['[', ']']) {
        return None;
    }
    let (host, port) = if let Some(v6) = authority.strip_prefix('[') {
        // Brackets only around an IPv6 literal.
        let (h, after) = v6.split_once(']')?;
        if !h.bytes().all(|b| b.is_ascii_hexdigit() || b == b':') {
            return None;
        }
        (h, if after.is_empty() { None } else { Some(after.strip_prefix(':')?) })
    } else {
        if authority.contains(['[', ']']) {
            return None;
        }
        authority.split_once(':').map_or((authority, None), |(h, p)| (h, Some(p)))
    };
    if port.is_some_and(|p| !is_port(p))
        || host.is_empty()
        || !host.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b':'))
    {
        return None;
    }
    let mut prefix = url.to_string();
    if !prefix.ends_with('/') {
        prefix.push('/');
    }
    Some(BaseUrl { https, host: host.to_ascii_lowercase(), prefix })
}

fn is_port(p: &str) -> bool {
    !p.is_empty() && p.len() <= 5 && p.bytes().all(|b| b.is_ascii_digit()) && p.parse::<u16>().is_ok()
}

/// Reads `hosts.toml` into the set of lowercase host names.
pub fn read_hosts(path: &Path) -> Result<BTreeSet<String>, ImportError> {
    let shown = path.display().to_string();
    let text = String::from_utf8(read_bounded(path, MAX_HOSTS_BYTES)?).map_err(|_| parse_err(&shown, "not UTF-8"))?;
    let table = text.parse::<toml::Table>().map_err(|e| parse_err(&shown, e.message()))?;
    let mut hosts = BTreeSet::new();
    for (key, value) in &table {
        if key != "host" {
            return Err(parse_err(&shown, format!("unknown top-level key `{key}`")));
        }
        let toml::Value::Array(items) = value else { return Err(parse_err(&shown, "`host` is not [[host]]")) };
        for (i, item) in items.iter().enumerate() {
            let toml::Value::Table(t) = item else {
                return Err(parse_err(&shown, format!("host #{} is not a table", i + 1)));
            };
            if let Some(k) = t.keys().find(|k| !matches!(k.as_str(), "name" | "purpose")) {
                return Err(parse_err(&shown, format!("host #{}: unknown key `{k}`", i + 1)));
            }
            let name = t.get("name").and_then(toml::Value::as_str).unwrap_or("");
            let purpose = t.get("purpose").and_then(toml::Value::as_str).unwrap_or("");
            if name.is_empty() || name.contains(['*', '/', ' ']) || !PURPOSES.contains(&purpose) {
                return Err(parse_err(&shown, format!("host #{}: bad `name` or `purpose`", i + 1)));
            }
            hosts.insert(name.to_ascii_lowercase());
        }
    }
    Ok(hosts)
}

/// Downloads `url` into `dst` with `curl`, refusing more than `cap` bytes; returns the bytes,
/// or the HTTP status or short reason for an `IMPORT-FETCH` finding.
fn download(url: &str, https: bool, cap: u64, dst: &Path) -> Result<Vec<u8>, String> {
    let fail = |status: &str| status.to_string();
    let proto = if https { "=https" } else { "=http" };
    let max = cap.to_string();
    // `-q` must come first: it makes curl ignore any ~/.curlrc.
    let out = Command::new("curl")
        .args(["-q", "--globoff", "--silent", "--show-error", "--max-redirs", "0"])
        .args(["--proto", proto, "--proto-redir", proto, "--connect-timeout", "30", "--max-time", "600"])
        .args(["--max-filesize", &max, "--write-out", "%{http_code}", "--output"])
        .arg(dst)
        .args(["--", url])
        .stdin(Stdio::null())
        .output()
        .map_err(|_| fail("curl-unavailable"))?;
    let code = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if !out.status.success() {
        return Err(fail(&format!("curl-exit-{}", out.status.code().unwrap_or(-1))));
    }
    if code != "200" {
        return Err(fail(if code.is_empty() { "no-status" } else { code.as_str() }));
    }
    read_bounded(dst, cap).map_err(|_| fail("too-large"))
}

/// Outcome of [`fetch`] beyond findings: the files written, for the caller's summary.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Fetched {
    pub entries: Vec<LockEntry>,
}

/// Fetches one release into `<site>/inputs/<tag>/` and updates the lock file. Returns findings
/// (exit 1) with nothing written, or the new lock entries.
pub fn fetch(args: &FetchArgs<'_>) -> Result<Result<Fetched, Vec<Finding>>, ImportError> {
    if !is_tag(args.release) {
        return Err(parse_err("--release", "must be 1 to 64 of A-Z a-z 0-9 . _ -"));
    }
    let Some(base) = parse_base_url(args.base_url) else {
        return Err(parse_err("--base-url", "must be an http:// or https:// URL without user, query or fragment"));
    };
    // Host check first: nothing is read from or sent to a host that is not allowlisted.
    let hosts = read_hosts(&args.site.join(HOSTS_FILE))?;
    if !hosts.contains(&base.host) {
        return Ok(Err(vec![Finding::Host { host: base.host }]));
    }
    if !base.https && !LOOPBACK.contains(&base.host.as_str()) {
        return Ok(Err(vec![Finding::Fetch { url: base.prefix, status: "http-not-loopback".into() }]));
    }
    // Current lock (absent means empty).
    let lock_path = args.site.join(LOCK_FILE);
    let (old, kind_findings) = if lock_path.exists() {
        let shown = lock_path.display().to_string();
        let text =
            String::from_utf8(read_bounded(&lock_path, MAX_LOCK_BYTES)?).map_err(|_| parse_err(&shown, "not UTF-8"))?;
        parse_lock(&text, &shown)?
    } else {
        (Vec::new(), Vec::new())
    };
    if !kind_findings.is_empty() {
        return Ok(Err(kind_findings));
    }

    let scratch = args.site.join(SCRATCH);
    let staged = match download_all(args.release, &base, &scratch) {
        Ok(Ok(staged)) => staged,
        Ok(Err(findings)) => {
            let _ = std::fs::remove_dir_all(&scratch);
            return Ok(Err(findings));
        }
        Err(e) => {
            let _ = std::fs::remove_dir_all(&scratch);
            return Err(e);
        }
    };
    let committed = commit(args.site, args.release, &scratch, &old, staged);
    let _ = std::fs::remove_dir_all(&scratch);
    committed.map(|entries| Ok(Fetched { entries }))
}

/// Downloaded files: (file name, bytes path in scratch, lock entry).
type Staged = Vec<(String, PathBuf, LockEntry)>;

fn download_all(tag: &str, base: &BaseUrl, scratch: &Path) -> Result<Result<Staged, Vec<Finding>>, ImportError> {
    if scratch.exists() {
        std::fs::remove_dir_all(scratch).map_err(|e| super::io_err(scratch, &e))?;
    }
    std::fs::create_dir_all(scratch).map_err(|e| super::io_err(scratch, &e))?;
    let url = format!("{}{RELEASE_MANIFEST}", base.prefix);
    let mpath = scratch.join(RELEASE_MANIFEST);
    let bytes = match download(&url, base.https, MAX_MANIFEST_BYTES, &mpath) {
        Ok(b) => b,
        Err(status) => return Ok(Err(vec![Finding::Fetch { url, status }])),
    };
    let rm = parse_release_manifest(&bytes, &url)?;
    if !tag_matches(tag, &rm.version) {
        return Ok(Err(vec![Finding::Release { name: RELEASE_MANIFEST.into(), tag: tag.into() }]));
    }
    let Some(date) = rm.date.clone() else {
        return Err(parse_err(&url, "release manifest: `date` missing or not a date"));
    };
    let entry = |name: &str, kind: &str, data: &[u8]| LockEntry {
        name: name.into(),
        kind: kind.into(),
        release: Some(tag.into()),
        source_rev: None,
        date: date.clone(),
        path: format!("inputs/{tag}/{name}"),
        sha256: hash_bytes(data),
    };
    let mut staged = vec![(RELEASE_MANIFEST.to_string(), mpath, entry(RELEASE_MANIFEST, "release_manifest", &bytes))];
    let mut findings = Vec::new();
    for name in REPORTS {
        let Some(expected) = rm.artefacts.get(name) else { continue };
        let url = format!("{}{name}", base.prefix);
        let path = scratch.join(name);
        match download(&url, base.https, MAX_REPORT_BYTES, &path) {
            Ok(data) => {
                let e = entry(name, "release_asset", &data);
                if e.sha256 == *expected {
                    staged.push((name.to_string(), path, e));
                } else {
                    findings.push(Finding::Digest { name: name.into(), expected: *expected, actual: e.sha256 });
                }
            }
            Err(status) => findings.push(Finding::Fetch { url, status }),
        }
    }
    if findings.is_empty() {
        Ok(Ok(staged))
    } else {
        findings.sort();
        Ok(Err(findings))
    }
}

/// Writes the new lock file to a temporary file, moves the files into `inputs/<tag>/`, then
/// replaces the lock file.
fn commit(
    site: &Path,
    tag: &str,
    scratch: &Path,
    old: &[LockEntry],
    staged: Staged,
) -> Result<Vec<LockEntry>, ImportError> {
    let new_paths: BTreeSet<&str> = staged.iter().map(|(_, _, e)| e.path.as_str()).collect();
    let mut merged: Vec<LockEntry> = old.iter().filter(|e| !new_paths.contains(e.path.as_str())).cloned().collect();
    merged.extend(staged.iter().map(|(_, _, e)| e.clone()));
    let lock_tmp = scratch.join(LOCK_FILE);
    std::fs::write(&lock_tmp, render_lock(&merged)).map_err(|e| super::io_err(&lock_tmp, &e))?;
    let dir = site.join("inputs").join(tag);
    std::fs::create_dir_all(&dir).map_err(|e| super::io_err(&dir, &e))?;
    for (name, src, _) in &staged {
        let dst = dir.join(name);
        std::fs::rename(src, &dst).map_err(|e| super::io_err(&dst, &e))?;
    }
    let lock = site.join(LOCK_FILE);
    std::fs::rename(&lock_tmp, &lock).map_err(|e| super::io_err(&lock, &e))?;
    let mut entries: Vec<LockEntry> = staged.into_iter().map(|(_, _, e)| e).collect();
    entries.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(entries)
}
