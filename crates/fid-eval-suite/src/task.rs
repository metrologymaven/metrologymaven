//! `task.json` (REQ-AGENT.EVAL.FORMAT-001, -004).

use crate::json::{index, is_id, join, parse_bounded, Ctx};
use crate::{ErrorCode, FormatError};
use fid_canon::CValue;
use fid_types::Digest;
use std::collections::BTreeSet;
use std::fmt;

/// A relative path with `/` separators: segments of `[A-Za-z0-9._-]`, never `.` or `..`,
/// at most 255 bytes. Absolute paths, drive letters and backslashes cannot be expressed.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RelPath(String);

impl RelPath {
    /// Parses a relative path; `None` if it is not one.
    pub fn parse(s: &str) -> Option<RelPath> {
        let ok = !s.is_empty()
            && s.len() <= 255
            && s.split('/').all(|seg| {
                !seg.is_empty()
                    && seg != "."
                    && seg != ".."
                    && seg.bytes().all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'_' | b'-'))
            });
        ok.then(|| RelPath(s.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for RelPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

macro_rules! wire_enum {
    ($(#[$m:meta])* $name:ident { $($var:ident = $s:literal),+ $(,)? }) => {
        $(#[$m])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub enum $name { $($var),+ }

        impl $name {
            /// Every value, in declaration order.
            pub const ALL: &'static [$name] = &[$($name::$var),+];

            /// The wire string.
            pub fn as_str(&self) -> &'static str {
                match self { $($name::$var => $s),+ }
            }

            /// Parses the wire string.
            pub fn parse(s: &str) -> Option<$name> {
                match s { $($s => Some($name::$var),)+ _ => None }
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(self.as_str())
            }
        }
    };
}
pub(crate) use wire_enum;

wire_enum! {
    /// Difficulty tier (REQ-AGENT.EVAL-007).
    Tier { SingleCommand = "single_command", MultiStep = "multi_step", FullJob = "full_job" }
}

wire_enum! {
    /// App tag (REQ-AGENT.EVAL-007).
    App { Inspect = "inspect", Probe = "probe", Network = "network", Report = "report" }
}

/// The start-state archive: a path relative to the task directory and its digest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartState {
    pub path: RelPath,
    pub digest: Digest,
}

/// Wall-time (seconds) and tool-call budgets, both at least 1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Budget {
    pub wall_s: u32,
    pub tool_calls: u32,
}

/// Read and write roots granted to the agent, relative to the sandbox.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Roots {
    pub read: Vec<RelPath>,
    pub write: Vec<RelPath>,
}

/// Licence of one input file: an SPDX licence ID or `project-authored`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Licence {
    pub file: RelPath,
    pub spdx: String,
}

/// A parsed `task.json`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Task {
    pub id: String,
    /// Source FLOW acceptance job (`JOB.*`).
    pub source_job: String,
    pub golden_digest: Digest,
    pub start_state: StartState,
    pub prompt: String,
    /// MCP server profile the agent gets.
    pub profile: String,
    pub budget: Budget,
    pub roots: Roots,
    pub tier: Tier,
    /// Non-empty, no repeats, in file order.
    pub apps: Vec<App>,
    /// Catalog workflow IDs the task exercises (zero or more).
    pub workflows: Vec<String>,
    /// Non-empty, one entry per input file.
    pub licences: Vec<Licence>,
}

const KEYS: &[&str] = &[
    "id",
    "source_job",
    "golden_digest",
    "start_state",
    "prompt",
    "profile",
    "budget",
    "roots",
    "tier",
    "apps",
    "workflows",
    "licences",
];

/// `project-authored` or a simple SPDX licence ID (`[A-Za-z0-9][A-Za-z0-9.+-]*`, at most 64).
fn is_spdx(s: &str) -> bool {
    if s == "project-authored" {
        return true;
    }
    let b = s.as_bytes();
    !b.is_empty()
        && b.len() <= 64
        && b[0].is_ascii_alphanumeric()
        && b.iter().all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'+' | b'-'))
}

fn relpath(cx: &mut Ctx, v: Option<&CValue>, field: &str) -> Option<RelPath> {
    let s = cx.string(v, field)?;
    let p = RelPath::parse(s);
    if p.is_none() {
        cx.err(field, "expected a relative path (segments of A-Z a-z 0-9 . _ -, no . or ..)");
    }
    p
}

fn relpaths(cx: &mut Ctx, v: Option<&CValue>, field: &str) -> Option<Vec<RelPath>> {
    let items = cx.arr(v, field)?;
    let mut out = Vec::new();
    let mut ok = true;
    for (i, item) in items.iter().enumerate() {
        let f = index(field, i);
        match relpath(cx, Some(item), &f) {
            Some(p) if out.contains(&p) => {
                cx.err(&f, "repeated root");
                ok = false;
            }
            Some(p) => out.push(p),
            None => ok = false,
        }
    }
    ok.then_some(out)
}

/// Parses a task file fail-closed, collecting every field error.
pub fn parse_task(bytes: &[u8], file: &str) -> Result<Task, Vec<FormatError>> {
    let root = parse_bounded(bytes, file, ErrorCode::InvalidTask).map_err(|e| vec![e])?;
    let mut cx = Ctx::new(ErrorCode::InvalidTask, file);
    let Some(m) = cx.obj(Some(&root), "", KEYS, KEYS) else {
        return Err(cx.finish());
    };

    let id = cx.string(m.get("id"), "id").map(str::to_string);
    let id = match id {
        Some(s) if is_id(&s) => Some(s),
        Some(_) => {
            cx.err("id", "expected [A-Za-z0-9][A-Za-z0-9._-]{0,63}");
            None
        }
        None => None,
    };
    let source_job = cx.text(m.get("source_job"), "source_job");
    let golden_digest = cx.digest(m.get("golden_digest"), "golden_digest");

    let start_state =
        cx.obj(m.get("start_state"), "start_state", &["path", "digest"], &["path", "digest"]).and_then(|s| {
            let path = relpath(&mut cx, s.get("path"), "start_state.path");
            let digest = cx.digest(s.get("digest"), "start_state.digest");
            Some(StartState { path: path?, digest: digest? })
        });

    let prompt = match cx.string(m.get("prompt"), "prompt") {
        Some("") => {
            cx.err("prompt", "must not be empty");
            None
        }
        other => other.map(str::to_string),
    };
    let profile = cx.text(m.get("profile"), "profile");

    let budget =
        cx.obj(m.get("budget"), "budget", &["wall_s", "tool_calls"], &["wall_s", "tool_calls"]).and_then(|b| {
            let wall_s = cx.pos_u32(b.get("wall_s"), "budget.wall_s");
            let tool_calls = cx.pos_u32(b.get("tool_calls"), "budget.tool_calls");
            Some(Budget { wall_s: wall_s?, tool_calls: tool_calls? })
        });

    let roots = cx.obj(m.get("roots"), "roots", &["read", "write"], &["read", "write"]).and_then(|r| {
        let read = relpaths(&mut cx, r.get("read"), "roots.read");
        let write = relpaths(&mut cx, r.get("write"), "roots.write");
        Some(Roots { read: read?, write: write? })
    });

    let tier = cx.string(m.get("tier"), "tier").and_then(|s| {
        let t = Tier::parse(s);
        if t.is_none() {
            cx.err("tier", format!("{s:?} is not one of single_command, multi_step, full_job"));
        }
        t
    });

    let apps = cx.arr(m.get("apps"), "apps").and_then(|items| {
        if items.is_empty() {
            cx.err("apps", "needs at least one app tag");
            return None;
        }
        let mut out = Vec::new();
        let mut ok = true;
        for (i, item) in items.iter().enumerate() {
            let f = index("apps", i);
            match cx.string(Some(item), &f).map(|s| (s, App::parse(s))) {
                Some((_, Some(a))) if out.contains(&a) => {
                    cx.err(&f, "repeated app tag");
                    ok = false;
                }
                Some((_, Some(a))) => out.push(a),
                Some((s, None)) => {
                    cx.err(&f, format!("{s:?} is not one of inspect, probe, network, report"));
                    ok = false;
                }
                None => ok = false,
            }
        }
        ok.then_some(out)
    });

    let workflows = cx.arr(m.get("workflows"), "workflows").and_then(|items| {
        let mut out: Vec<String> = Vec::new();
        let mut ok = true;
        for (i, item) in items.iter().enumerate() {
            let f = index("workflows", i);
            match cx.text(Some(item), &f) {
                Some(w) if out.contains(&w) => {
                    cx.err(&f, "repeated workflow");
                    ok = false;
                }
                Some(w) => out.push(w),
                None => ok = false,
            }
        }
        ok.then_some(out)
    });

    let licences = cx.arr(m.get("licences"), "licences").and_then(|items| {
        if items.is_empty() {
            cx.err("licences", "every input file needs a licence entry");
            return None;
        }
        let mut out = Vec::new();
        let mut seen = BTreeSet::new();
        let mut ok = true;
        for (i, item) in items.iter().enumerate() {
            let f = index("licences", i);
            let Some(l) = cx.obj(Some(item), &f, &["file", "spdx"], &["file", "spdx"]) else {
                ok = false;
                continue;
            };
            let file = relpath(&mut cx, l.get("file"), &join(&f, "file"));
            if let Some(p) = &file {
                if !seen.insert(p.clone()) {
                    cx.err(&join(&f, "file"), "repeated licence entry");
                    ok = false;
                }
            }
            let spdx = cx.string(l.get("spdx"), &join(&f, "spdx")).map(str::to_string);
            if let Some(s) = &spdx {
                if !is_spdx(s) {
                    cx.err(&join(&f, "spdx"), "expected an SPDX licence ID or project-authored");
                    ok = false;
                    continue;
                }
            }
            match (file, spdx) {
                (Some(file), Some(spdx)) => out.push(Licence { file, spdx }),
                _ => ok = false,
            }
        }
        ok.then_some(out)
    });

    if cx.has_errors() {
        return Err(cx.finish());
    }
    let (
        Some(id),
        Some(source_job),
        Some(golden_digest),
        Some(start_state),
        Some(prompt),
        Some(profile),
        Some(budget),
        Some(roots),
        Some(tier),
        Some(apps),
        Some(workflows),
        Some(licences),
    ) = (id, source_job, golden_digest, start_state, prompt, profile, budget, roots, tier, apps, workflows, licences)
    else {
        return Err(cx.finish());
    };
    Ok(Task {
        id,
        source_job,
        golden_digest,
        start_state,
        prompt,
        profile,
        budget,
        roots,
        tier,
        apps,
        workflows,
        licences,
    })
}
