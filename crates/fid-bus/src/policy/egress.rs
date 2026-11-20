//! Egress primitive and network guard. Spec node `CORE.BUS.POL.EGRESS`.
//!
//! `egress.send@1` (X1, determinism N, projection `request_only`) is the only way project
//! data leaves the machine (CONSTITUTION §3.1, D-031). It sends one artifact already staged
//! in the content-addressed store to one destination an administrator configured, and only
//! with a human approval recorded for that invocation.
//!
//! **Fail closed.** Everything not explicitly allowed is refused, and every refusal is an
//! error of the bus catalogue (never `internal`):
//!
//! - `policy/bad_destination`: the destination ID is malformed or not configured, or this
//!   build has no transport for its kind (REQ-003).
//! - `policy/no_approval`: no [`EgressApproval`], or one that does not name the executing
//!   human, the artifact digest, the destination ID and the destination's configuration
//!   digest, or one older than [`APPROVAL_MAX_AGE`]; a principal that is neither `human`,
//!   `agent` nor `script` (REQ-002).
//! - `validation/bad_artifact`: the artifact is not in the store, is larger than one blob,
//!   or its bytes do not hash to the digest (REQ-003).
//! - `validation/schema`: parameters with unknown or missing fields, a malformed digest, or
//!   a note that is too long or holds control, format or other forbidden characters.
//! - `policy/class_forbidden`: a network permit asked for a class other than X1, I1 or D1,
//!   or an egress sent under a permit not issued for X1.
//! - `policy/not_on_mcp`, `policy/bad_approval`, `not_found/unknown_request`: from the
//!   projection check (`policy::projection`), which runs first.
//!
//! **Destinations** ([`Destinations`]) come only from the A0 admin setting
//! [`DESTINATIONS_KEY`] (machine scope), read through `fid_store::settings::resolve`;
//! nothing in this module adds or widens one. The caller names a destination by ID only,
//! and IDs are matched byte for byte against a lowercase ASCII grammar, so two IDs never
//! differ only in case or by a lookalike character. An `https` URL is accepted only in a
//! strict form ([`HttpsUrl::parse`]): lowercase `https://`, no user info, query or
//! fragment, a host that is a lowercase LDH DNS name without IDN (`xn--`) or reserved
//! (`??--`) labels or a canonical dotted IPv4 address, an optional port without leading
//! zeros, and a path of unreserved ASCII characters without `.`/`..` or empty segments.
//! The accepted text is exactly the configured text: nothing is normalised. Each
//! destination has a configuration digest, and an approval is bound to it, so an
//! administrator changing a destination after approval makes the approval void
//! (`no_approval`) instead of silently sending elsewhere.
//!
//! **Principal.** The principal is derived by the bus from the channel
//! ([`SessionTable::derive_principal`]); parameters are `{artifact, destination, note?}`
//! and anything else is `validation/schema`, so no parameter can name an approver, a
//! principal or a URL. An `agent` (any channel) or `script` call becomes a pending request
//! (`policy::projection::Filing`); only a `human` with an approval from the approval dialog
//! ([`EgressApproval::ask`], which refuses every non-human and every MCP channel) sends.
//! A human approving an agent's request needs both the request approval (`approval_of`)
//! and a fresh [`EgressApproval`] for the same artifact and destination.
//!
//! **Sending** ([`EgressSender::send`]): the artifact is read from the store and verified
//! twice (the store's own check, then SHA-256 of the exact bytes to be sent); the
//! transport is called exactly once, under a [`NetPermit`] issued for X1 (`issue_permit`,
//! crate-private, called by the pipeline); the outcome (`sent`, `failed`, `cancelled`) is
//! recorded in one [`EGRESS_EVENT`] event with the digest, size, destination and approval,
//! and the flag `already_present` (`true` when the destination already held exactly these
//! bytes and nothing was written). A failure or a cancellation is recorded, never retried
//! (REQ-006).
//!
//! **Redirects and name resolution.** The spec does not cover them; this module never
//! follows a redirect itself (it has no HTTP client), and the [`Transport`] contract
//! requires an `https` transport to treat any redirect as `failed` and to connect only to
//! the configured host. This build ships no `https` transport (see the PR for the deferred
//! TLS stack), so an `https` destination is refused with `bad_destination` until one is
//! given to [`Transports`]; `network_root` destinations are written by
//! [`NetworkRootTransport`].

#![deny(clippy::wildcard_enum_match_arm, clippy::match_wildcard_for_single_variants)]

use crate::error::{BusError, Detail, PolicyReason, Untrusted, ValidationReason};
use crate::handler::{CancelToken, Event, EventType, HandlerCtx, StateView, ValidParams};
use crate::policy::fs::{open_checked, windows_device};
use crate::policy::projection::{
    authorise as authorise_projection_call, forbidden_in_reason, is_mcp_channel, Approval, Decision, Filing, RequestId,
};
use crate::principal::{
    Channel, ConfirmMethod, OsUser, PresenceOutcome, Principal, PrincipalKind, PrincipalRef, SessionTable, UpgradeClock,
};
use cap_std::fs::Dir;
use fid_canon::CValue;
use fid_journal::EntryTime;
use fid_registry::{CommandDef, DefError};
use fid_store::cas::{BlobStore, CasError, CHUNK_SIZE};
use fid_store::settings::{
    machine_config_dir, resolve as resolve_settings, user_config_dir, FileStore, Scope, SettingWarning,
    SettingsRegistry, Sources,
};
use fid_types::{Digest, SideEffect};
use std::collections::hash_map::RandomState;
use std::collections::BTreeMap;
use std::fmt;
use std::fs::{self, OpenOptions};
use std::hash::{BuildHasher, Hasher};
use std::io::{self, Read, Write};
use std::marker::PhantomData;
use std::path::{Component, Path, PathBuf, Prefix};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

/// `egress.send@1`.
pub const SEND_COMMAND: &str = "egress.send";
/// The event of an egress attempt.
pub const EGRESS_EVENT: &str = "EgressSend";
/// The admin setting that holds the destinations (CORE.STO.SET, A0, machine scope).
pub const DESTINATIONS_KEY: &str = "bus.egress_destinations";
/// Most destinations the setting may hold.
pub const MAX_DESTINATIONS: usize = 64;
/// Longest destination ID, in bytes.
pub const MAX_DESTINATION_ID_BYTES: usize = 64;
/// Longest `https` URL, in bytes.
pub const MAX_URL_BYTES: usize = 2048;
/// Longest `network_root` path, in bytes.
pub const MAX_ROOT_PATH_BYTES: usize = 4096;
/// Longest note, in characters (Unicode scalar values).
pub const MAX_NOTE_CHARS: usize = 500;
/// Longest remote receipt or failure text kept, in characters.
pub const MAX_RECEIPT_CHARS: usize = 500;
/// Largest artifact sent: one store blob ([`CHUNK_SIZE`]); chunked objects are not sent yet.
pub const MAX_ARTIFACT_BYTES: u64 = CHUNK_SIZE;
/// How long the approval dialog waits for the person.
pub const APPROVAL_DEADLINE: Duration = Duration::from_secs(120);
/// Oldest approval [`authorise`] accepts, by the monotonic clock (maintainer decision on #336).
pub const APPROVAL_MAX_AGE: Duration = Duration::from_secs(300);

const LIFECYCLE_SINCE: &str = "2026-11-20";

fid_store::fid_settings! {
    pub fn egress_settings;
    {
        key: "bus.egress_destinations",
        owner: "bus",
        schema: r#"{
            "type": "array",
            "maxItems": 64,
            "items": {
                "type": "object",
                "additionalProperties": false,
                "required": ["id", "kind"],
                "properties": {
                    "id": {"type": "string", "minLength": 1, "maxLength": 64},
                    "kind": {"type": "string", "enum": ["network_root", "https"]},
                    "path": {"type": "string", "minLength": 1, "maxLength": 4096},
                    "url": {"type": "string", "minLength": 1, "maxLength": 2048},
                    "method": {"type": "string", "enum": ["PUT", "POST"]}
                }
            }
        }"#,
        default: "[]",
        scopes: [Machine],
        admin: true,
    }
}

// ------------------------------------------------------------------ errors

fn policy(reason: PolicyReason, what: &str) -> BusError {
    BusError::Policy(reason, Detail::new(what))
}

fn bad_destination(what: &str) -> BusError {
    policy(PolicyReason::BadDestination, what)
}

fn no_approval(what: &str) -> BusError {
    policy(PolicyReason::NoApproval, what)
}

fn bad_artifact(what: &str) -> BusError {
    BusError::Validation(ValidationReason::BadArtifact, Detail::new(what))
}

fn schema(what: &str) -> BusError {
    BusError::Validation(ValidationReason::Schema, Detail::new(what))
}

fn text(s: &str) -> CValue {
    CValue::Str(s.to_string())
}

fn obj<const N: usize>(fields: [(&str, CValue); N]) -> CValue {
    CValue::Obj(fields.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

// ------------------------------------------------------------------ destination IDs

/// A destination ID: 1 to [`MAX_DESTINATION_ID_BYTES`] lowercase ASCII letters, digits, `-`
/// or `_`, starting with a letter or digit. Compared byte for byte.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DestinationId(String);

impl DestinationId {
    /// Checks an ID; `None` if it breaks the grammar.
    pub fn new(id: &str) -> Option<DestinationId> {
        let b = id.as_bytes();
        let low = |c: &u8| c.is_ascii_lowercase() || c.is_ascii_digit();
        let ok = !b.is_empty()
            && b.len() <= MAX_DESTINATION_ID_BYTES
            && b.first().is_some_and(low)
            && b.iter().all(|c| low(c) || *c == b'-' || *c == b'_');
        ok.then(|| DestinationId(id.to_string()))
    }

    /// The ID text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for DestinationId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

// ------------------------------------------------------------------ https URLs

/// An `https` destination URL in the strict form described in the module documentation.
/// The text is exactly what was configured.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpsUrl {
    text: String,
    host: String,
    port: u16,
    path: String,
}

fn ipv4_part(p: &str) -> Option<u8> {
    let ok =
        !p.is_empty() && p.len() <= 3 && p.bytes().all(|b| b.is_ascii_digit()) && (p == "0" || !p.starts_with('0'));
    if ok {
        p.parse::<u8>().ok()
    } else {
        None
    }
}

fn check_host(host: &str) -> Result<(), String> {
    if host.is_empty() {
        return Err("host is empty".into());
    }
    if host.bytes().all(|b| b.is_ascii_digit() || b == b'.') {
        let parts: Vec<&str> = host.split('.').collect();
        if parts.len() == 4 && parts.iter().all(|p| ipv4_part(p).is_some()) {
            return Ok(());
        }
        return Err("numeric host is not a canonical dotted IPv4 address".into());
    }
    if host.len() > 253 {
        return Err("host is longer than 253 bytes".into());
    }
    let labels: Vec<&str> = host.split('.').collect();
    for l in &labels {
        let b = l.as_bytes();
        if b.is_empty() || b.len() > 63 {
            return Err("host label is empty or longer than 63 bytes".into());
        }
        if !b.iter().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'-') {
            return Err("host labels may hold only lowercase ASCII letters, digits and '-'".into());
        }
        if b.first() == Some(&b'-') || b.last() == Some(&b'-') {
            return Err("host label starts or ends with '-'".into());
        }
        if b.len() >= 4 && b[2] == b'-' && b[3] == b'-' {
            return Err("host label is an IDN (xn--) or reserved (??--) label".into());
        }
    }
    if labels.last().is_some_and(|l| l.bytes().all(|c| c.is_ascii_digit())) {
        return Err("host's last label is numeric".into());
    }
    Ok(())
}

fn check_path(path: &str) -> Result<(), String> {
    if !path.starts_with('/') {
        return Err("path must start with '/'".into());
    }
    let ok_char = |c: u8| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'.' | b'_' | b'~' | b'/');
    if !path.bytes().all(ok_char) {
        return Err("path may hold only ASCII letters, digits, '-', '.', '_', '~' and '/'".into());
    }
    let segs: Vec<&str> = path[1..].split('/').collect();
    let n = segs.len();
    for (i, s) in segs.iter().enumerate() {
        if s.is_empty() && i + 1 != n {
            return Err("path has an empty segment".into());
        }
        if *s == "." || *s == ".." {
            return Err("path has a '.' or '..' segment".into());
        }
    }
    Ok(())
}

impl HttpsUrl {
    /// Checks a configured URL; the error says which rule it breaks.
    pub fn parse(s: &str) -> Result<HttpsUrl, String> {
        if s.is_empty() || s.len() > MAX_URL_BYTES {
            return Err(format!("URL must be 1 to {MAX_URL_BYTES} bytes"));
        }
        if !s.bytes().all(|b| (0x21..=0x7e).contains(&b)) {
            return Err("URL may hold only printable ASCII without spaces".into());
        }
        let Some(rest) = s.strip_prefix("https://") else {
            return Err("URL must start with lowercase https://".into());
        };
        if let Some(c) = rest.chars().find(|c| matches!(c, '@' | '?' | '#' | '\\' | '%')) {
            return Err(format!("URL may not hold {c:?} (user info, query, fragment or escapes)"));
        }
        let (authority, path) = match rest.find('/') {
            Some(i) => (&rest[..i], &rest[i..]),
            None => return Err("URL must have a path starting with '/'".into()),
        };
        if authority.contains('[') || authority.contains(']') {
            return Err("IPv6 hosts are not supported".into());
        }
        let (host, port) = match authority.split_once(':') {
            None => (authority, 443),
            Some((h, p)) => {
                let digits = !p.is_empty() && p.len() <= 5 && p.bytes().all(|b| b.is_ascii_digit());
                if !digits || p.starts_with('0') {
                    return Err("port must be 1 to 65535 without leading zeros".into());
                }
                let port = p.parse::<u16>().map_err(|_| "port must be 1 to 65535".to_string())?;
                (h, port)
            }
        };
        check_host(host)?;
        check_path(path)?;
        Ok(HttpsUrl { text: s.to_string(), host: host.to_string(), port, path: path.to_string() })
    }

    /// The URL, exactly as configured.
    pub fn as_str(&self) -> &str {
        &self.text
    }

    /// The host (a DNS name or dotted IPv4 address).
    pub fn host(&self) -> &str {
        &self.host
    }

    /// The port (443 unless given).
    pub fn port(&self) -> u16 {
        self.port
    }

    /// The path.
    pub fn path(&self) -> &str {
        &self.path
    }
}

/// The HTTP method of an `https` destination.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HttpMethod {
    Put,
    Post,
}

impl HttpMethod {
    /// The wire string (`PUT`, `POST`).
    pub const fn as_str(self) -> &'static str {
        match self {
            HttpMethod::Put => "PUT",
            HttpMethod::Post => "POST",
        }
    }
}

// ------------------------------------------------------------------ destinations

/// Where a destination sends to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DestinationKind {
    /// A folder on a network share; the artifact is written as a new file named by its
    /// digest's hex.
    NetworkRoot { path: PathBuf },
    /// An HTTPS endpoint.
    Https { url: HttpsUrl, method: HttpMethod },
}

impl DestinationKind {
    /// The wire name of the kind.
    pub const fn as_str(&self) -> &'static str {
        match self {
            DestinationKind::NetworkRoot { .. } => "network_root",
            DestinationKind::Https { .. } => "https",
        }
    }
}

/// One configured destination, with the digest of its configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Destination {
    id: DestinationId,
    kind: DestinationKind,
    digest: Digest,
}

impl Destination {
    pub fn id(&self) -> &DestinationId {
        &self.id
    }

    pub fn kind(&self) -> &DestinationKind {
        &self.kind
    }

    /// The `sha256:` digest of the destination's canonical configuration
    /// (`{id, kind, path}` or `{id, kind, method, url}`); an approval is bound to it.
    pub fn digest(&self) -> Digest {
        self.digest
    }

    /// What the approval dialog shows as the target: the URL or the folder path.
    pub fn target(&self) -> String {
        match &self.kind {
            DestinationKind::NetworkRoot { path } => path.display().to_string(),
            DestinationKind::Https { url, .. } => url.as_str().to_string(),
        }
    }

    fn config_value(id: &DestinationId, kind: &DestinationKind) -> CValue {
        match kind {
            DestinationKind::NetworkRoot { path } => obj([
                ("id", text(id.as_str())),
                ("kind", text("network_root")),
                ("path", text(&path.to_string_lossy())),
            ]),
            DestinationKind::Https { url, method } => obj([
                ("id", text(id.as_str())),
                ("kind", text("https")),
                ("method", text(method.as_str())),
                ("url", text(url.as_str())),
            ]),
        }
    }
}

/// Why the destinations could not be read. Fail closed: the caller must then use
/// [`Destinations::default`] (none), so every send gets `bad_destination`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DestinationsError(String);

impl fmt::Display for DestinationsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "egress destinations setting {DESTINATIONS_KEY}: {}", self.0)
    }
}

impl std::error::Error for DestinationsError {}

/// A read-only snapshot of the configured destinations. There is no API to add or change
/// one; a snapshot is built only from the setting's value.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Destinations {
    map: BTreeMap<DestinationId, Destination>,
}

impl Destinations {
    /// Reads a value of [`DESTINATIONS_KEY`] directly. Test hook only, behind the
    /// `permit-test-hooks` feature, which only `[dev-dependencies]` may enable (checked by
    /// `tests/test_hooks.rs`); production code reads destinations through
    /// [`Destinations::from_machine_settings`].
    #[cfg(feature = "permit-test-hooks")]
    #[doc(hidden)]
    pub fn from_value(v: &CValue) -> Result<Destinations, DestinationsError> {
        Destinations::parse_value(v)
    }

    /// The whole value is refused on any error: not an array, more than
    /// [`MAX_DESTINATIONS`] items, an item that is not an object, an unknown field or a field
    /// of the other kind, a malformed or duplicate ID, an unknown kind, a `network_root`
    /// path that breaks [`check_root_path`], an `https` URL that breaks
    /// [`HttpsUrl::parse`], or a method other than `PUT` or `POST`.
    fn parse_value(v: &CValue) -> Result<Destinations, DestinationsError> {
        let err = |s: String| Err(DestinationsError(s));
        let CValue::Arr(items) = v else { return err("must be an array".into()) };
        if items.len() > MAX_DESTINATIONS {
            return err("too many destinations".into());
        }
        let mut map = BTreeMap::new();
        for (i, item) in items.iter().enumerate() {
            let CValue::Obj(o) = item else { return err(format!("destination {i} must be an object")) };
            let id = match o.get("id") {
                Some(CValue::Str(s)) => DestinationId::new(s),
                _ => None,
            };
            let Some(id) = id else { return err(format!("destination {i}: id is missing or malformed")) };
            let allowed: &[&str] = match o.get("kind") {
                Some(CValue::Str(k)) if k == "network_root" => &["id", "kind", "path"],
                Some(CValue::Str(k)) if k == "https" => &["id", "kind", "method", "url"],
                _ => return err(format!("destination {id}: kind must be network_root or https")),
            };
            if let Some(k) = o.keys().find(|k| !allowed.contains(&k.as_str())) {
                let k: String = k.chars().take(32).filter(|c| c.is_ascii_graphic()).collect();
                return err(format!("destination {id} has unknown or misplaced field {k:?}"));
            }
            let kind = if allowed.contains(&"path") {
                let checked = match o.get("path") {
                    Some(CValue::Str(s)) => check_root_path(s).map(|()| PathBuf::from(s)),
                    _ => Err("path is missing".to_string()),
                };
                match checked {
                    Ok(path) => DestinationKind::NetworkRoot { path },
                    Err(why) => return err(format!("destination {id}: {why}")),
                }
            } else {
                let url = match o.get("url") {
                    Some(CValue::Str(s)) => HttpsUrl::parse(s),
                    _ => Err("url is missing".to_string()),
                };
                let url = match url {
                    Ok(u) => u,
                    Err(why) => return err(format!("destination {id}: {why}")),
                };
                let method = match o.get("method") {
                    Some(CValue::Str(m)) if m == "PUT" => HttpMethod::Put,
                    Some(CValue::Str(m)) if m == "POST" => HttpMethod::Post,
                    _ => return err(format!("destination {id}: method must be PUT or POST")),
                };
                DestinationKind::Https { url, method }
            };
            if map.contains_key(&id) {
                return err(format!("duplicate destination id {id}"));
            }
            let digest = fid_canon::digest(&Destination::config_value(&id, &kind))
                .map(|d| Digest::from_bytes(d.0))
                .map_err(|e| DestinationsError(format!("destination {id} has no canonical form: {}", e.code())))?;
            map.insert(id.clone(), Destination { id, kind, digest });
        }
        Ok(Destinations { map })
    }

    /// Reads [`DESTINATIONS_KEY`] through the settings layer (CORE.STO.SET, read only), with
    /// the warnings of the read. The declaration is this module's own ([`egress_settings`]:
    /// admin, machine scope only), so a value stored at user or project scope is ignored.
    pub fn from_settings(sources: &Sources<'_>) -> Result<(Destinations, Vec<SettingWarning>), DestinationsError> {
        let registry = SettingsRegistry::build(&egress_settings())
            .map_err(|_| DestinationsError("the built-in declaration does not build".into()))?;
        match registry.get(DESTINATIONS_KEY) {
            Some(d) if d.admin() && !d.allows(Scope::User) && !d.allows(Scope::Project) => {}
            _ => return Err(DestinationsError("the built-in declaration is not an admin machine setting".into())),
        }
        let (values, warnings) = resolve_settings(&registry, sources, |d| d.key() == DESTINATIONS_KEY);
        let value = values
            .into_iter()
            .find(|r| r.key == DESTINATIONS_KEY)
            .ok_or_else(|| DestinationsError("not resolved".into()))?;
        Ok((Destinations::parse_value(&value.value)?, warnings))
    }

    /// The production path: reads [`DESTINATIONS_KEY`] from this machine's real settings
    /// files, located by `fid_store::settings::machine_config_dir` (Windows
    /// `%ProgramData%`, macOS `/Library/Application Support`, other Unix `/etc`, then
    /// `fiducial/settings.json`) and `user_config_dir` for the user scope, whose values of
    /// this key are ignored. Fails closed when the machine folder cannot be located; the
    /// caller then uses [`Destinations::default`] (none).
    pub fn from_machine_settings() -> Result<(Destinations, Vec<SettingWarning>), DestinationsError> {
        let os = std::env::consts::OS;
        let env = |k: &str| std::env::var_os(k);
        let machine_path = machine_config_dir(os, env)
            .ok_or_else(|| DestinationsError("the machine settings folder is unknown".into()))?;
        // The user scope may not hold this key; when its folder is unknown, an empty path is
        // read as a missing file.
        let user_path = user_config_dir(os, env).unwrap_or_default();
        let machine = FileStore::new(Scope::Machine, machine_path)
            .map_err(|_| DestinationsError("the machine settings file cannot be opened".into()))?;
        let user = FileStore::new(Scope::User, user_path)
            .map_err(|_| DestinationsError("the user settings file cannot be opened".into()))?;
        Destinations::from_settings(&Sources { project: None, user: &user, machine: &machine })
    }

    /// The destination with exactly this ID text; `bad_destination` if the text is not a
    /// valid ID or no destination has it.
    pub fn lookup(&self, id: &str) -> Result<&Destination, BusError> {
        let shown: String = id.chars().take(64).filter(|c| c.is_ascii_graphic()).collect();
        DestinationId::new(id)
            .and_then(|i| self.map.get(&i))
            .ok_or_else(|| bad_destination(&format!("destination {shown:?} is not configured")))
    }

    /// Every destination, by ID.
    pub fn iter(&self) -> impl Iterator<Item = &Destination> {
        self.map.values()
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
}

/// The rules for a `network_root` path: at most [`MAX_ROOT_PATH_BYTES`] bytes, no NUL or
/// other control character, absolute on this platform, no `.` or `..` segment (split on `/`
/// and `\\`), no Windows device or verbatim path (`\\.\\`, `\\?\\`, `\\??\\`, also
/// with `/`), and only a drive letter or a UNC share as a Windows prefix.
///
/// Every component (split on `/` and `\\`, the UNC server and share names included, a
/// leading drive `X:` excepted) must also be a name Windows keeps as written, on every
/// platform, since a share may be served by Windows: no trailing `.` or space (Windows strips
/// them, so the name would alias another), no `:` (an alternate data stream), and no
/// reserved device name (`CON`, `PRN`, `AUX`, `NUL`, `COM0`-`COM9`, `LPT0`-`LPT9`, `CONIN$`,
/// `CONOUT$`, `CLOCK$` and the superscript-digit forms, in any case and with any extension;
/// the rule of `policy::fs`).
pub fn check_root_path(s: &str) -> Result<(), String> {
    if s.is_empty() || s.len() > MAX_ROOT_PATH_BYTES {
        return Err(format!("path must be 1 to {MAX_ROOT_PATH_BYTES} bytes"));
    }
    if s.chars().any(char::is_control) {
        return Err("path holds a control character".into());
    }
    let lead: String = s.chars().take(4).map(|c| if c == '/' { '\\' } else { c }).collect();
    if lead.starts_with("\\\\.\\") || lead.starts_with("\\\\?\\") || lead.starts_with("\\??\\") {
        return Err("device and verbatim paths are refused".into());
    }
    if s.split(['/', '\\']).any(|seg| seg == "." || seg == "..") {
        return Err("path has a '.' or '..' segment".into());
    }
    for (i, seg) in s.split(['/', '\\']).enumerate() {
        let b = seg.as_bytes();
        let drive = i == 0 && b.len() == 2 && b[0].is_ascii_alphabetic() && b[1] == b':';
        if seg.is_empty() || drive {
            continue;
        }
        if seg.ends_with('.') || seg.ends_with(' ') {
            return Err("a path component ends with '.' or a space".into());
        }
        if seg.contains(':') {
            return Err("a path component holds ':'".into());
        }
        if windows_device(seg) {
            return Err("a path component is a Windows reserved device name".into());
        }
    }
    let p = Path::new(s);
    if !p.is_absolute() {
        return Err("path must be absolute".into());
    }
    for c in p.components() {
        match c {
            Component::Prefix(pre) => match pre.kind() {
                Prefix::Disk(_) | Prefix::UNC(_, _) => {}
                Prefix::Verbatim(_) | Prefix::VerbatimUNC(_, _) | Prefix::VerbatimDisk(_) | Prefix::DeviceNS(_) => {
                    return Err("device and verbatim paths are refused".into())
                }
            },
            Component::RootDir | Component::Normal(_) => {}
            Component::CurDir | Component::ParentDir => return Err("path has a '.' or '..' component".into()),
        }
    }
    Ok(())
}

// ------------------------------------------------------------------ parameters

/// The parameters of `egress.send@1`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SendParams {
    artifact: Digest,
    destination: String,
    note: Option<Untrusted<String>>,
}

impl SendParams {
    /// Reads `{artifact, destination, note?}`; anything else, a malformed digest, a
    /// non-string destination, or a note longer than [`MAX_NOTE_CHARS`] or holding a
    /// forbidden character (`policy::projection::forbidden_in_reason`) is
    /// `validation/schema`. The destination text is checked against the configuration
    /// later (`bad_destination`).
    pub fn parse(v: &CValue) -> Result<SendParams, BusError> {
        let CValue::Obj(o) = v else { return Err(schema("parameters must be an object")) };
        if let Some(k) = o.keys().find(|k| !matches!(k.as_str(), "artifact" | "destination" | "note")) {
            let k: String = k.chars().take(64).collect();
            return Err(schema(&format!("parameters have unknown field {k:?}")));
        }
        let artifact = match o.get("artifact") {
            Some(CValue::Str(s)) => Digest::parse(s).map_err(|_| schema("artifact must be a sha256: digest"))?,
            _ => return Err(schema("parameters lack the artifact digest")),
        };
        let destination = match o.get("destination") {
            Some(CValue::Str(s)) => s.clone(),
            _ => return Err(schema("parameters lack the destination ID")),
        };
        let note = match o.get("note") {
            None => None,
            Some(CValue::Str(s)) => {
                if s.chars().count() > MAX_NOTE_CHARS {
                    return Err(schema(&format!("note is longer than {MAX_NOTE_CHARS} characters")));
                }
                if let Some(c) = s.chars().find(|c| forbidden_in_reason(*c)) {
                    return Err(schema(&format!("note holds a forbidden character U+{:04X}", u32::from(c))));
                }
                Some(Untrusted::new(s.clone()))
            }
            Some(_) => return Err(schema("note must be a string")),
        };
        Ok(SendParams { artifact, destination, note })
    }

    pub fn artifact(&self) -> Digest {
        self.artifact
    }

    /// The destination ID text as passed.
    pub fn destination(&self) -> &str {
        &self.destination
    }

    pub fn note(&self) -> Option<&Untrusted<String>> {
        self.note.as_ref()
    }
}

// ------------------------------------------------------------------ approval

/// What the approval dialog shows the person.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EgressPrompt {
    pub artifact: Digest,
    pub destination: DestinationId,
    /// `network_root` or `https`.
    pub kind: &'static str,
    /// The URL or folder the bytes go to.
    pub target: String,
    /// `PUT` or `POST` for `https`.
    pub method: Option<&'static str>,
}

/// The approval dialog of a desktop app (implemented by the apps): shows the artifact
/// digest and the destination and asks the person to confirm. Must give up after
/// `deadline`.
pub trait ApprovalDialog {
    fn confirm(&self, prompt: &EgressPrompt, deadline: Duration) -> PresenceOutcome;
}

/// A human approval of one egress, from the approval dialog. Built only by
/// [`EgressApproval::ask`]; not `Clone`, and consumed by [`authorise`], so it approves one
/// invocation.
#[derive(Debug, PartialEq, Eq)]
pub struct EgressApproval {
    approver: PrincipalRef,
    artifact: Digest,
    destination: DestinationId,
    destination_digest: Digest,
    at: EntryTime,
    os_user: OsUser,
    method: ConfirmMethod,
}

impl EgressApproval {
    /// Shows the approval dialog for sending `artifact` to `destination` and returns the
    /// approval if the person confirmed. The approver is derived from `channel` by `table`
    /// and nowhere else. `policy/no_approval` when the caller is not `human`, the channel is
    /// an MCP channel, the channel knows no OS user, the person declined or did not answer
    /// before [`APPROVAL_DEADLINE`], or the confirming OS user is not the channel's.
    pub fn ask(
        table: &SessionTable,
        channel: &dyn Channel,
        dialog: &dyn ApprovalDialog,
        clock: &mut dyn UpgradeClock,
        artifact: Digest,
        destination: &Destination,
    ) -> Result<EgressApproval, BusError> {
        let principal = table.derive_principal(channel)?;
        if principal.kind() != PrincipalKind::Human || is_mcp_channel(channel.kind()) {
            return Err(no_approval(&format!(
                "only a human outside MCP can approve an egress; caller is {}",
                principal.kind().as_str()
            )));
        }
        let Some(expected) = channel.authenticated_identity().and_then(|i| i.os_user) else {
            return Err(no_approval("the channel knows no OS user to confirm the approval"));
        };
        let prompt = EgressPrompt {
            artifact,
            destination: destination.id.clone(),
            kind: destination.kind.as_str(),
            target: destination.target(),
            method: match &destination.kind {
                DestinationKind::NetworkRoot { .. } => None,
                DestinationKind::Https { method, .. } => Some(method.as_str()),
            },
        };
        match dialog.confirm(&prompt, APPROVAL_DEADLINE) {
            PresenceOutcome::Confirmed { os_user, method } if os_user == expected => Ok(EgressApproval {
                approver: principal.reference(),
                artifact,
                destination: destination.id.clone(),
                destination_digest: destination.digest,
                at: clock.now(),
                os_user,
                method,
            }),
            PresenceOutcome::Confirmed { .. } => Err(no_approval("the approval was confirmed by another OS user")),
            PresenceOutcome::Declined => Err(no_approval("the person declined the egress")),
            PresenceOutcome::TimedOut => Err(no_approval("the approval dialog timed out")),
        }
    }

    pub fn approver(&self) -> &PrincipalRef {
        &self.approver
    }

    pub fn artifact(&self) -> Digest {
        self.artifact
    }

    pub fn destination(&self) -> &DestinationId {
        &self.destination
    }

    /// The configuration digest of the destination the person approved.
    pub fn destination_digest(&self) -> Digest {
        self.destination_digest
    }

    /// When the person confirmed.
    pub fn at(&self) -> &EntryTime {
        &self.at
    }

    fn to_value(&self) -> CValue {
        obj([
            ("approved_by", self.approver.to_value()),
            ("at", text(&self.at.utc)),
            ("method", text(self.method.as_str())),
            ("os_user", text(self.os_user.as_str())),
        ])
    }
}

// ------------------------------------------------------------------ authorise

/// An egress cleared to send: built only by [`authorise`].
#[derive(Debug)]
pub struct Cleared {
    principal: Principal,
    approval: EgressApproval,
    destination: Destination,
    artifact: Digest,
    note: Option<Untrusted<String>>,
    request: Option<Approval>,
}

impl Cleared {
    /// The approving human.
    pub fn principal(&self) -> &Principal {
        &self.principal
    }

    pub fn destination(&self) -> &Destination {
        &self.destination
    }

    pub fn artifact(&self) -> Digest {
        self.artifact
    }

    /// The approval of a pending request this send carries out, if any; the commit must
    /// carry its `RequestApproved` event (`Approval::event`).
    pub fn request_approval(&self) -> Option<&Approval> {
        self.request.as_ref()
    }
}

/// What [`authorise`] decided.
#[derive(Debug)]
pub enum EgressDecision {
    /// Send now ([`EgressSender::send`]).
    Send(Box<Cleared>),
    /// Do not send: file a pending request (`Filing::commit`).
    File(Box<Filing>),
}

/// The authorise stage for `egress.send@1`. The principal is derived from `channel` by
/// `table`; `approval_of` and `consent` come from the invocation, never from the
/// parameters. Order:
///
/// 1. `def` must be `egress.send@1` (X1), else `no_approval`; parameters
///    ([`SendParams::parse`], `validation/schema`); the destination must be configured
///    (`bad_destination`), for every principal, before anything is filed.
/// 2. The projection, approval and class checks (`policy::projection::authorise`): an MCP
///    or `agent` call files a request; an `approval_of` from anyone but a human outside MCP
///    is `bad_approval`.
/// 3. A call the projection lets run: `agent` or `script` files a request (REQ-002);
///    `service`, `device` and `system` are `no_approval`; a `human` needs `consent` naming
///    that human, the artifact, the destination ID and its current configuration digest
///    (`no_approval` otherwise).
///
/// 4. The approval must be at most [`APPROVAL_MAX_AGE`] old at `now` by the monotonic
///    clock: a stale approval is no approval (`no_approval`, also when `now` is before the
///    approval).
///
/// Pure: sends nothing and commits nothing.
#[allow(clippy::too_many_arguments)]
pub fn authorise(
    table: &SessionTable,
    channel: &dyn Channel,
    def: &CommandDef,
    params: &ValidParams,
    approval_of: Option<&RequestId>,
    consent: Option<EgressApproval>,
    destinations: &Destinations,
    view: &dyn StateView,
    now: &EntryTime,
) -> Result<(Principal, EgressDecision), BusError> {
    if def.id != SEND_COMMAND || def.major != 1 || def.side_effect != SideEffect::X1 {
        return Err(no_approval(&format!("{}@{} is not the egress primitive", def.id, def.major)));
    }
    let p = SendParams::parse(params.value())?;
    let destination = destinations.lookup(&p.destination)?;
    let (principal, decision) = authorise_projection_call(table, channel, def, params, approval_of, view)?.into_parts();
    let request = match decision {
        Decision::File(f) => return Ok((principal, EgressDecision::File(Box::new(f)))),
        Decision::RunApproved(a) => Some(a),
        Decision::Run => match principal.kind() {
            PrincipalKind::Agent | PrincipalKind::Script => {
                let f = Filing::new(&principal, def, params, view)?;
                return Ok((principal, EgressDecision::File(Box::new(f))));
            }
            PrincipalKind::Human => None,
            PrincipalKind::Service | PrincipalKind::Device | PrincipalKind::System => {
                return Err(no_approval(&format!(
                    "a {} principal cannot send data off the machine",
                    principal.kind().as_str()
                )));
            }
        },
    };
    if principal.kind() != PrincipalKind::Human {
        return Err(no_approval("only a human may send"));
    }
    let Some(approval) = consent else {
        return Err(no_approval("egress.send@1 needs an approval from the approval dialog for this invocation"));
    };
    if approval.approver != principal.reference() {
        return Err(no_approval("the approval was given by another principal"));
    }
    if approval.artifact != p.artifact {
        return Err(no_approval("the approval names another artifact"));
    }
    if approval.destination != destination.id {
        return Err(no_approval("the approval names another destination"));
    }
    if approval.destination_digest != destination.digest {
        return Err(no_approval("the destination's configuration changed after the approval"));
    }
    let max_age = u64::try_from(APPROVAL_MAX_AGE.as_nanos()).unwrap_or(u64::MAX);
    match now.mono_ns.checked_sub(approval.at.mono_ns) {
        Some(age) if age <= max_age => {}
        Some(_) | None => {
            return Err(no_approval(&format!(
                "the approval is older than {} seconds (or dated after now)",
                APPROVAL_MAX_AGE.as_secs()
            )))
        }
    }
    let cleared = Cleared {
        principal: principal.clone(),
        approval,
        destination: destination.clone(),
        artifact: p.artifact,
        note: p.note,
        request,
    };
    Ok((principal, EgressDecision::Send(Box::new(cleared))))
}

// ------------------------------------------------------------------ network permit

/// Whether a command of `class` may hold a [`NetPermit`]: X1 (egress), I1 (instrument I/O)
/// and D1 (device I/O) only (CONSTITUTION §3.1, REQ-CORE.BUS.POL-008). Total, no wildcard.
pub const fn permits_network(class: SideEffect) -> bool {
    match class {
        SideEffect::X1 | SideEffect::I1 | SideEffect::D1 => true,
        SideEffect::R0
        | SideEffect::R1
        | SideEffect::W1
        | SideEffect::W2
        | SideEffect::W3
        | SideEffect::M1
        | SideEffect::S1
        | SideEffect::A0 => false,
    }
}

/// The execution a permit belongs to. The pipeline creates one per command execution and
/// drops it when the handler returns; a permit borrows it mutably for its whole life, so no
/// permit outlives it, and it issues at most one permit. Not `Clone`.
#[derive(Debug, Default)]
pub struct PermitScope {
    issued: bool,
}

impl PermitScope {
    /// A scope that has issued no permit.
    pub fn new() -> PermitScope {
        PermitScope { issued: false }
    }

    /// Whether this scope already issued its permit.
    pub fn issued(&self) -> bool {
        self.issued
    }
}

/// Permission to open network connections for one execution of an X1, I1 or D1 command
/// (REQ-004). Every socket-opening function of the `fid-net` facade takes one. Only this
/// crate can make one (`issue_permit` is crate-private): private fields, not `Clone`,
/// not `Copy`, and it lives no longer than its [`PermitScope`].
pub struct NetPermit<'s> {
    class: SideEffect,
    _scope: PhantomData<&'s mut PermitScope>,
}

impl fmt::Debug for NetPermit<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NetPermit").field("class", &self.class.as_str()).finish()
    }
}

impl NetPermit<'_> {
    /// The class the permit was issued for (X1, I1 or D1).
    pub fn class(&self) -> SideEffect {
        self.class
    }
}

/// Issues the one network permit of an execution of `def` (REQ-004): only for an X1, I1 or
/// D1 command ([`permits_network`]) and at most once per `scope`; otherwise
/// `policy/class_forbidden` and the scope is unchanged. Crate-private: the pipeline
/// (CORE.BUS.PIPE.STAGE) calls it for the handler it is about to run, so no other crate can
/// mint a permit.
#[allow(dead_code)] // first production caller: CORE.BUS.PIPE.STAGE
pub(crate) fn issue_permit<'s>(def: &CommandDef, scope: &'s mut PermitScope) -> Result<NetPermit<'s>, BusError> {
    if scope.issued || !permits_network(def.side_effect) {
        return Err(policy(
            PolicyReason::ClassForbidden,
            &format!(
                "{}@{} is {}; only an executing X1, I1 or D1 command gets one network permit",
                def.id,
                def.major,
                def.side_effect.as_str()
            ),
        ));
    }
    scope.issued = true;
    Ok(NetPermit { class: def.side_effect, _scope: PhantomData })
}

/// Test-only access to `issue_permit` for integration tests outside this crate; not part
/// of the API. Exists only with the `permit-test-hooks` feature, which only
/// `[dev-dependencies]` may enable (checked by `tests/test_hooks.rs`).
#[cfg(feature = "permit-test-hooks")]
#[doc(hidden)]
pub fn issue_permit_for_tests<'s>(def: &CommandDef, scope: &'s mut PermitScope) -> Result<NetPermit<'s>, BusError> {
    issue_permit(def, scope)
}

// ------------------------------------------------------------------ transports

/// What a transport observed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SendOutcome {
    /// The destination accepted all the bytes; with the remote's receipt, untrusted.
    Sent { receipt: Option<Untrusted<String>> },
    /// The destination already held exactly these bytes and nothing was written (a
    /// `network_root` file of the digest's name with the same content). Recorded as `sent`
    /// with `already_present: true` in the event.
    AlreadyPresent,
    /// The send failed; why, untrusted.
    Failed { reason: Untrusted<String> },
    /// Cancelled before completion.
    Cancelled,
}

/// Sends bytes to one kind of destination. Contract: send `bytes` once to exactly the
/// configured target of `destination` (no retry, no other host); for `https`, use the
/// configured method, verify the server certificate, connect only to the configured host
/// and port, and treat any redirect (3xx) as `Failed` without following it; open sockets
/// only through `fid_net` with `permit`; stop and return `Cancelled` when `cancel` is set.
pub trait Transport: Send + Sync {
    fn send(
        &self,
        permit: &NetPermit<'_>,
        destination: &Destination,
        artifact: &Digest,
        bytes: &[u8],
        cancel: &CancelToken,
    ) -> SendOutcome;
}

/// The transports of this build, by destination kind. A kind without a transport is
/// refused with `bad_destination` before anything is read or sent.
#[derive(Clone, Copy, Default)]
pub struct Transports<'a> {
    pub network_root: Option<&'a dyn Transport>,
    pub https: Option<&'a dyn Transport>,
}

/// Writes the artifact into a `network_root` folder under the name of the digest's 64 hex
/// digits, atomically and without replacing anything:
///
/// 1. write `<hex>.partial-<random>` (created with `create_new`) and sync it;
/// 2. hard-link it to `<hex>`, which fails if `<hex>` exists, then remove the partial file,
///    and hash `<hex>` again: `sent` if it holds exactly the artifact, `failed` otherwise (the
///    file was changed after the link; it is left as it is);
/// 3. if `<hex>` already exists, hash it: [`SendOutcome::AlreadyPresent`] if it holds exactly
///    the artifact, `failed` otherwise (it is never replaced).
///
/// `<hex>` is hashed only if it is a regular file by `symlink_metadata`, and it is opened
/// without following a link (`O_NOFOLLOW`; `FILE_FLAG_OPEN_REPARSE_POINT`, then refused if
/// the handle is a reparse point) and without blocking (`O_NONBLOCK`, so a FIFO swapped in
/// after the check cannot hang the send), through the root folder held as a capability
/// (`policy::fs`). A link, junction, FIFO, device or folder of that name is `failed`.
///
/// The partial file is removed on every path. The folder must exist. A share that does not
/// support hard links gives `failed`.
#[derive(Debug, Clone, Copy, Default)]
pub struct NetworkRootTransport {
    fail_after: Option<usize>,
    tamper_after_link: bool,
}

static PARTIAL_COUNTER: AtomicU64 = AtomicU64::new(0);

fn partial_suffix() -> String {
    let mut h = RandomState::new().build_hasher();
    h.write_u64(PARTIAL_COUNTER.fetch_add(1, Ordering::Relaxed));
    h.write_u32(std::process::id());
    format!("{:016x}", h.finish())
}

/// Whether the file `name` in the folder `root` holds exactly `digest` (at most
/// [`MAX_ARTIFACT_BYTES`] + 1 bytes read). `Err` says why not: not a plain regular file
/// (a link, junction or other reparse point, FIFO, device or folder), unreadable, or other
/// content. Never follows a link and never blocks on a FIFO (see [`NetworkRootTransport`]).
fn holds(root: &Path, name: &str, digest: &Digest) -> Result<(), String> {
    let dir = Dir::open_ambient_dir(root, cap_std::ambient_authority())
        .map_err(|e| format!("the network root cannot be opened: {:?}", e.kind()))?;
    let file = open_checked(&dir, Path::new(name), false)
        .map_err(|e| format!("the file of that name is not a plain regular file ({}/{})", e.code(), e.reason()))?;
    let mut bytes = Vec::new();
    file.take(MAX_ARTIFACT_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("the file of that name cannot be read: {:?}", e.kind()))?;
    if Digest::from_bytes(fid_canon::digest_bytes(&bytes).0) == *digest {
        Ok(())
    } else {
        Err("the file of that name holds other content".into())
    }
}

impl NetworkRootTransport {
    /// The transport.
    pub const fn new() -> NetworkRootTransport {
        NetworkRootTransport { fail_after: None, tamper_after_link: false }
    }

    /// Test hook: a transport whose write fails with an I/O error after `bytes` bytes.
    #[cfg(feature = "permit-test-hooks")]
    #[doc(hidden)]
    pub const fn failing_after_for_tests(bytes: usize) -> NetworkRootTransport {
        NetworkRootTransport { fail_after: Some(bytes), tamper_after_link: false }
    }

    /// Test hook: a transport that overwrites `<hex>` right after placing it, as another
    /// writer on the share could, so the check after the link sees other content.
    #[cfg(feature = "permit-test-hooks")]
    #[doc(hidden)]
    pub const fn tampering_after_link_for_tests() -> NetworkRootTransport {
        NetworkRootTransport { fail_after: None, tamper_after_link: true }
    }

    fn tamper(&self, target: &Path) {
        if self.tamper_after_link {
            let _ = fs::write(target, b"changed by another writer after the link");
        }
    }

    fn write_partial(&self, file: &mut fs::File, bytes: &[u8]) -> io::Result<()> {
        let written = match self.fail_after {
            Some(n) if n < bytes.len() => file
                .write_all(&bytes[..n])
                .and_then(|()| Err(io::Error::other("simulated failure in the middle of the write"))),
            Some(_) | None => file.write_all(bytes),
        };
        written.and_then(|()| file.sync_all())
    }
}

impl Transport for NetworkRootTransport {
    fn send(
        &self,
        _permit: &NetPermit<'_>,
        destination: &Destination,
        artifact: &Digest,
        bytes: &[u8],
        cancel: &CancelToken,
    ) -> SendOutcome {
        let failed = |why: String| SendOutcome::Failed { reason: Untrusted::new(why) };
        let DestinationKind::NetworkRoot { path } = &destination.kind else {
            return failed("not a network_root destination".into());
        };
        match fs::metadata(path) {
            Ok(m) if m.is_dir() => {}
            Ok(_) => return failed("the network root is not a folder".into()),
            Err(e) => return failed(format!("the network root cannot be reached: {:?}", e.kind())),
        }
        if cancel.is_cancelled() {
            return SendOutcome::Cancelled;
        }
        let name = artifact.hex();
        let target = path.join(name);
        let partial = path.join(format!("{name}.partial-{}", partial_suffix()));
        let mut file = match OpenOptions::new().write(true).create_new(true).open(&partial) {
            Ok(f) => f,
            Err(e) => return failed(format!("cannot create the partial file: {:?}", e.kind())),
        };
        let written = self.write_partial(&mut file, bytes);
        drop(file);
        if let Err(e) = written {
            let _ = fs::remove_file(&partial);
            return failed(format!("write failed: {:?}", e.kind()));
        }
        let linked = fs::hard_link(&partial, &target);
        let _ = fs::remove_file(&partial);
        match linked {
            Ok(()) => {
                self.tamper(&target);
                match holds(path, name, artifact) {
                    Ok(()) => SendOutcome::Sent { receipt: None },
                    Err(why) => failed(format!("the placed file does not verify; it was left as it is: {why}")),
                }
            }
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => match holds(path, name, artifact) {
                Ok(()) => SendOutcome::AlreadyPresent,
                Err(why) => failed(format!("a file of that name exists and was not replaced: {why}")),
            },
            Err(e) => failed(format!("cannot place the file: {:?}", e.kind())),
        }
    }
}

// ------------------------------------------------------------------ sending

/// The recorded outcome of one send.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Sent,
    Failed,
    Cancelled,
}

impl Outcome {
    pub const ALL: [Outcome; 3] = [Outcome::Sent, Outcome::Failed, Outcome::Cancelled];

    pub const fn as_str(self) -> &'static str {
        match self {
            Outcome::Sent => "sent",
            Outcome::Failed => "failed",
            Outcome::Cancelled => "cancelled",
        }
    }
}

/// What one send produced: the events to commit (one [`EGRESS_EVENT`]), the command result
/// `{outcome, bytes, remote_receipt?}`, and the request approval to commit with them.
#[derive(Debug)]
pub struct Sent {
    pub outcome: Outcome,
    pub events: Vec<Event>,
    pub result: CValue,
    pub request_approval: Option<Approval>,
}

/// Keeps at most [`MAX_RECEIPT_CHARS`] characters of untrusted remote text, without
/// forbidden characters.
fn bounded(t: &Untrusted<String>) -> String {
    t.as_untrusted().chars().filter(|c| !forbidden_in_reason(*c)).take(MAX_RECEIPT_CHARS).collect()
}

/// Sends cleared egresses.
pub struct EgressSender<'a> {
    store: &'a BlobStore,
    transports: Transports<'a>,
}

impl<'a> EgressSender<'a> {
    pub fn new(store: &'a BlobStore, transports: Transports<'a>) -> Self {
        EgressSender { store, transports }
    }

    /// The artifact's own bytes, through `BlobStore::read_object` (CORE.STO.CAS REQ-010): the
    /// digest of a chunked object gives the object, never its manifest, so a manifest is
    /// never sent as if it were the data (maintainer decision on #336). At most
    /// [`MAX_ARTIFACT_BYTES`]: a larger artifact, which every chunked object this store
    /// writes is, is refused before it is read.
    fn load(&self, digest: &Digest) -> Result<Vec<u8>, BusError> {
        self.store.read_object(digest, MAX_ARTIFACT_BYTES).map_err(|e| match e {
            CasError::Integrity(_) => bad_artifact(&format!("artifact {digest} does not verify")),
            CasError::NotFound(_) => bad_artifact(&format!("artifact {digest} is not in the store")),
            CasError::OutOfRange { .. } => {
                bad_artifact(&format!("artifact {digest} is larger than one blob ({MAX_ARTIFACT_BYTES} bytes)"))
            }
            CasError::ManifestInvalid(_) | CasError::Io(_) => {
                bad_artifact(&format!("artifact {digest} cannot be read: {}", e.code()))
            }
        })
    }

    /// Sends a cleared egress under `permit` (REQ-001, REQ-006): `class_forbidden` unless
    /// the permit is for X1; `no_approval` unless `ctx`'s principal is the approving human;
    /// `bad_destination` if the destination is no longer in `current` (the setting as read
    /// now) and `no_approval` if its configuration digest changed since the approval;
    /// `bad_destination` when this build has no transport for the kind; `bad_artifact`
    /// when the artifact is missing, larger than one blob, does not verify, or names a chunk
    /// manifest whose object cannot be verified (a manifest is never sent). Nothing is
    /// sent on any of these. Then the transport is called exactly once (not at all when
    /// cancellation was already requested) and the outcome is recorded.
    pub fn send(
        &self,
        cleared: Cleared,
        current: &Destinations,
        permit: &NetPermit<'_>,
        ctx: &HandlerCtx,
    ) -> Result<Sent, BusError> {
        if permit.class() != SideEffect::X1 {
            return Err(policy(PolicyReason::ClassForbidden, "egress needs a permit issued for X1"));
        }
        if ctx.principal().reference() != cleared.principal.reference() {
            return Err(no_approval("the executing principal is not the approving human"));
        }
        if current.lookup(cleared.destination.id.as_str())?.digest != cleared.destination.digest {
            return Err(no_approval("the destination's configuration changed after the approval"));
        }
        let transport = match &cleared.destination.kind {
            DestinationKind::NetworkRoot { .. } => self.transports.network_root,
            DestinationKind::Https { .. } => self.transports.https,
        };
        let Some(transport) = transport else {
            return Err(bad_destination(&format!(
                "this build has no transport for {} destinations",
                cleared.destination.kind.as_str()
            )));
        };
        let bytes = self.load(&cleared.artifact)?;
        let observed = if ctx.cancel().is_cancelled() {
            SendOutcome::Cancelled
        } else {
            transport.send(permit, &cleared.destination, &cleared.artifact, &bytes, ctx.cancel())
        };
        let size = bytes.len() as f64; // at most CHUNK_SIZE, exact
        let (outcome, receipt, failure, already_present) = match &observed {
            SendOutcome::Sent { receipt } => (Outcome::Sent, receipt.as_ref().map(bounded), None, false),
            SendOutcome::AlreadyPresent => (Outcome::Sent, None, None, true),
            SendOutcome::Failed { reason } => (Outcome::Failed, None, Some(bounded(reason)), false),
            SendOutcome::Cancelled => (Outcome::Cancelled, None, None, false),
        };
        let mut body = BTreeMap::new();
        // Distinct from a write: `true` only when the destination already held exactly these
        // bytes and nothing was written (outcome `sent`).
        body.insert("already_present".to_string(), CValue::Bool(already_present));
        body.insert("approval".to_string(), cleared.approval.to_value());
        body.insert("artifact".to_string(), text(cleared.artifact.as_str()));
        body.insert("bytes".to_string(), CValue::Num(size));
        body.insert("destination".to_string(), text(cleared.destination.id.as_str()));
        body.insert("destination_digest".to_string(), text(cleared.destination.digest.as_str()));
        body.insert("kind".to_string(), text(cleared.destination.kind.as_str()));
        body.insert("outcome".to_string(), text(outcome.as_str()));
        if let Some(f) = &failure {
            body.insert("failure".to_string(), text(f));
        }
        if let Some(r) = &receipt {
            body.insert("remote_receipt".to_string(), text(r));
        }
        if let Some(n) = &cleared.note {
            body.insert("note".to_string(), text(n.as_untrusted()));
        }
        if let Some(a) = &cleared.request {
            body.insert("request".to_string(), text(&a.request().to_string()));
        }
        let event = Event::new(EventType::new(EGRESS_EVENT)?, CValue::Obj(body));
        let mut result = BTreeMap::new();
        result.insert("outcome".to_string(), text(outcome.as_str()));
        result.insert("bytes".to_string(), CValue::Num(size));
        if let Some(r) = receipt {
            result.insert("remote_receipt".to_string(), CValue::Str(r));
        }
        Ok(Sent { outcome, events: vec![event], result: CValue::Obj(result), request_approval: cleared.request })
    }
}

// ------------------------------------------------------------------ definition

fn strs(items: &[&str]) -> CValue {
    CValue::Arr(items.iter().map(|s| text(s)).collect())
}

fn object(required: &[&str], properties: CValue) -> CValue {
    obj([
        ("type", text("object")),
        ("additionalProperties", CValue::Bool(false)),
        ("required", strs(required)),
        ("properties", properties),
    ])
}

/// The `egress.send@1` definition, built through `CommandDef::from_value` so every field
/// rule of the registry applies.
pub fn command_defs() -> Result<Vec<CommandDef>, DefError> {
    let params = object(
        &["artifact", "destination"],
        obj([
            (
                "artifact",
                obj([
                    ("type", text("string")),
                    ("pattern", text(Digest::PATTERN)),
                    ("description", text("Digest of an artifact staged in the store.")),
                ]),
            ),
            (
                "destination",
                obj([
                    ("type", text("string")),
                    ("minLength", CValue::Num(1.0)),
                    ("maxLength", CValue::Num(64.0)),
                    ("pattern", text("^[a-z0-9][a-z0-9_-]*$")),
                    ("description", text("ID of a destination configured by an administrator.")),
                ]),
            ),
            (
                "note",
                obj([
                    ("type", text("string")),
                    ("maxLength", CValue::Num(500.0)),
                    ("x-untrusted", CValue::Bool(true)),
                    ("description", text("Optional note, untrusted text, at most 500 characters.")),
                ]),
            ),
        ]),
    );
    let outcomes: Vec<&str> = Outcome::ALL.iter().map(|o| o.as_str()).collect();
    let result = object(
        &["bytes", "outcome"],
        obj([
            ("outcome", obj([("enum", strs(&outcomes))])),
            ("bytes", obj([("type", text("integer")), ("minimum", CValue::Num(0.0))])),
            (
                "remote_receipt",
                obj([
                    ("type", text("string")),
                    ("maxLength", CValue::Num(500.0)),
                    ("x-untrusted", CValue::Bool(true)),
                    ("description", text("What the destination returned, untrusted text.")),
                ]),
            ),
        ]),
    );
    let def = obj([
        ("id", text(SEND_COMMAND)),
        ("major", CValue::Num(1.0)),
        ("title", text("Send an artifact off the machine")),
        (
            "description",
            text(
                "Sends one staged artifact to one destination configured by an administrator, only with a \
                 human approval for this invocation. Agent and script calls become pending requests.",
            ),
        ),
        ("params_schema", params),
        ("result_schema", result),
        ("side_effect", text("X1")),
        ("mcp_projection", text("request_only")),
        ("determinism", text("N")),
        ("idempotency", text("none")),
        ("fs_effect", text("none")),
        ("undo", text("not_undoable")),
        ("preconditions", CValue::Arr(Vec::new())),
        ("effects", strs(&[EGRESS_EVENT])),
        ("algorithms", CValue::Arr(Vec::new())),
        ("exposure", strs(&["cli", "gui", "mcp"])),
        ("lifecycle", obj([("state", text("active")), ("since", text(LIFECYCLE_SINCE))])),
        ("human_only", CValue::Bool(false)),
        ("network", text("egress")),
        ("web", text("available")),
        ("cli_bare", CValue::Bool(false)),
    ]);
    Ok(vec![CommandDef::from_value(def)?])
}
