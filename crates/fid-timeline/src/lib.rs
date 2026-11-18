//! PR-timeline evaluator (spec node `ECO.COMM.TIMELINE`): the author-blocked clock.
//!
//! A pure function from a recorded PR event timeline, a config and `now` to the action
//! due: `none`, `remind` or `propose-close`. It reads only its inputs (no forge API, no
//! clock, no environment), and all time arithmetic is in integer UTC seconds, so the
//! result is D0 given `now`. Posting the comment or label is a thin workflow elsewhere
//! (ECO.CI.WF). Repository tooling, layer `tool`.
//!
//! Rules (REQ-ECO.COMM.TIMELINE-001..004):
//! - Events are processed in (time, file order). Events after `now` are ignored.
//! - The author becomes the blocker when a maintainer submits `review_changes_requested`
//!   or adds the `waiting-on-author` label. The author stops being the blocker when the
//!   author pushes (`commit_pushed`), comments (`comment`) or re-requests review
//!   (`review_requested`), or when anyone removes the `waiting-on-author` label.
//! - Events by accounts on the bot list never change the blocker state.
//! - The clock is the sum of all author-blocked intervals, up to `now`, in elapsed
//!   calendar seconds (no business-hours or holiday exclusions: the spec defines none).
//! - `bot_reminder_posted` and `bot_close_proposed` are idempotence markers, honoured
//!   whoever posted them. A reminder counts until the author next stops being the
//!   blocker; a closure proposal counts forever.
//! - While the author is the blocker: `propose-close` if the clock is at least
//!   `close_days` and no closure proposal exists; else `remind` if the clock is at least
//!   `remind_days` and no reminder counts; else `none`. While the author is not the
//!   blocker the action is always `none`.

mod config;
mod time;

pub use config::{parse_config, Config, DEFAULT_CLOSE_DAYS, DEFAULT_REMIND_DAYS, MAX_CONFIG_BYTES, MAX_DAYS};
pub use time::{parse_timestamp, SECONDS_PER_DAY};

use fid_canon::CValue;
use std::collections::BTreeMap;
use std::fmt;

/// Largest event file accepted, in bytes.
pub const MAX_INPUT_BYTES: usize = 16 * 1024 * 1024;
/// Most events accepted in one timeline.
pub const MAX_EVENTS: usize = 100_000;
/// The label that makes the author the blocker.
pub const WAITING_LABEL: &str = "waiting-on-author";

/// Stable error codes (spec node Design: Errors).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCode {
    /// Malformed event file, timestamp or `--now`.
    Input,
    /// Malformed or incomplete config.
    Config,
}

impl ErrorCode {
    pub fn as_str(self) -> &'static str {
        match self {
            ErrorCode::Input => "TIMELINE-INPUT",
            ErrorCode::Config => "TIMELINE-CONFIG",
        }
    }
}

/// An error with a stable code and a human-readable message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimelineError {
    pub code: ErrorCode,
    pub message: String,
}

impl TimelineError {
    pub fn input(message: impl Into<String>) -> Self {
        TimelineError { code: ErrorCode::Input, message: message.into() }
    }

    pub fn config(message: impl Into<String>) -> Self {
        TimelineError { code: ErrorCode::Config, message: message.into() }
    }
}

impl fmt::Display for TimelineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code.as_str(), self.message)
    }
}

impl std::error::Error for TimelineError {}

/// Event kinds (spec node Design: Data structures).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Kind {
    Opened,
    ReviewChangesRequested,
    ReviewApproved,
    ReviewCommented,
    LabelAdded,
    LabelRemoved,
    CommitPushed,
    Comment,
    ReviewRequested,
    BotReminderPosted,
    BotCloseProposed,
}

impl Kind {
    pub const ALL: [Kind; 11] = [
        Kind::Opened,
        Kind::ReviewChangesRequested,
        Kind::ReviewApproved,
        Kind::ReviewCommented,
        Kind::LabelAdded,
        Kind::LabelRemoved,
        Kind::CommitPushed,
        Kind::Comment,
        Kind::ReviewRequested,
        Kind::BotReminderPosted,
        Kind::BotCloseProposed,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Opened => "opened",
            Kind::ReviewChangesRequested => "review_changes_requested",
            Kind::ReviewApproved => "review_approved",
            Kind::ReviewCommented => "review_commented",
            Kind::LabelAdded => "label_added",
            Kind::LabelRemoved => "label_removed",
            Kind::CommitPushed => "commit_pushed",
            Kind::Comment => "comment",
            Kind::ReviewRequested => "review_requested",
            Kind::BotReminderPosted => "bot_reminder_posted",
            Kind::BotCloseProposed => "bot_close_proposed",
        }
    }

    pub fn parse(s: &str) -> Option<Kind> {
        Kind::ALL.into_iter().find(|k| k.as_str() == s)
    }

    /// Whether events of this kind carry a `label` field.
    pub fn has_label(self) -> bool {
        matches!(self, Kind::LabelAdded | Kind::LabelRemoved)
    }
}

/// One recorded timeline event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Event {
    /// UTC seconds since 1970-01-01T00:00:00Z.
    pub at: i64,
    pub actor: String,
    pub kind: Kind,
    /// The label name, present exactly for `label_added` and `label_removed`.
    pub label: Option<String>,
}

fn field<'a>(obj: &'a BTreeMap<String, CValue>, i: usize, key: &str) -> Result<&'a str, TimelineError> {
    match obj.get(key) {
        Some(CValue::Str(s)) => Ok(s),
        Some(_) => Err(TimelineError::input(format!("event {i}: {key} must be a string"))),
        None => Err(TimelineError::input(format!("event {i}: missing {key}"))),
    }
}

/// Parses an event file: a JSON array of `{"at", "actor", "kind"}` objects, plus
/// `"label"` on `label_added` / `label_removed`. Unknown keys and kinds are rejected.
pub fn parse_events(input: &[u8]) -> Result<Vec<Event>, TimelineError> {
    if input.len() > MAX_INPUT_BYTES {
        return Err(TimelineError::input(format!("event file is larger than {MAX_INPUT_BYTES} bytes")));
    }
    let value = fid_canon::parse(input)
        .map_err(|e| TimelineError::input(format!("event file is not valid JSON ({})", e.code())))?;
    let CValue::Arr(items) = value else {
        return Err(TimelineError::input("event file must be a JSON array"));
    };
    if items.len() > MAX_EVENTS {
        return Err(TimelineError::input(format!("more than {MAX_EVENTS} events")));
    }
    let mut events = Vec::with_capacity(items.len());
    for (i, item) in items.iter().enumerate() {
        let CValue::Obj(obj) = item else {
            return Err(TimelineError::input(format!("event {i} must be an object")));
        };
        if let Some(k) = obj.keys().find(|k| !matches!(k.as_str(), "at" | "actor" | "kind" | "label")) {
            return Err(TimelineError::input(format!("event {i}: unknown key {k:?}")));
        }
        let at = parse_timestamp(field(obj, i, "at")?)
            .map_err(|e| TimelineError::input(format!("event {i}: {}", e.message)))?;
        let actor = field(obj, i, "actor")?;
        if actor.trim().is_empty() {
            return Err(TimelineError::input(format!("event {i}: empty actor")));
        }
        let kind_s = field(obj, i, "kind")?;
        let kind =
            Kind::parse(kind_s).ok_or_else(|| TimelineError::input(format!("event {i}: unknown kind {kind_s:?}")))?;
        let label = if kind.has_label() {
            Some(field(obj, i, "label")?.to_string())
        } else if obj.contains_key("label") {
            return Err(TimelineError::input(format!("event {i}: label is only allowed on label events")));
        } else {
            None
        };
        events.push(Event { at, actor: actor.to_string(), kind, label });
    }
    Ok(events)
}

/// The action due now (REQ-ECO.COMM.TIMELINE-004).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    None,
    Remind,
    ProposeClose,
}

impl Action {
    pub fn as_str(self) -> &'static str {
        match self {
            Action::None => "none",
            Action::Remind => "remind",
            Action::ProposeClose => "propose-close",
        }
    }
}

/// Result of [`evaluate`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Evaluation {
    /// Author-blocked seconds up to `now` (REQ-003).
    pub clock_seconds: i64,
    /// `clock_seconds` in whole days, rounded down.
    pub clock_days: i64,
    pub action: Action,
    /// Whether the author is the blocker at `now`.
    pub author_is_blocker: bool,
    /// The author login used (from the config, or the first `opened` event).
    pub author: String,
}

impl Evaluation {
    /// The tool's JSON output: `{"clock_days":<int>,"action":"<action>"}`.
    pub fn to_json(&self) -> String {
        format!("{{\"clock_days\":{},\"action\":\"{}\"}}", self.clock_days, self.action.as_str())
    }
}

/// Evaluates a timeline at `now` (UTC seconds). Errors with `TIMELINE-CONFIG` when the
/// config names no author and the timeline has no `opened` event.
pub fn evaluate(events: &[Event], config: &Config, now: i64) -> Result<Evaluation, TimelineError> {
    let mut order: Vec<usize> = (0..events.len()).collect();
    order.sort_by_key(|&i| (events[i].at, i));

    let author = match &config.author {
        Some(a) => a.clone(),
        None => order
            .iter()
            .map(|&i| &events[i])
            .find(|e| e.kind == Kind::Opened)
            .map(|e| e.actor.clone())
            .ok_or_else(|| TimelineError::config("no author in the config and no `opened` event in the timeline"))?,
    };

    let mut blocked_since: Option<i64> = None;
    let mut clock: i64 = 0;
    let mut reminded = false;
    let mut proposed = false;
    for &i in &order {
        let e = &events[i];
        if e.at > now {
            break;
        }
        match e.kind {
            Kind::BotReminderPosted => {
                reminded = true;
                continue;
            }
            Kind::BotCloseProposed => {
                proposed = true;
                continue;
            }
            _ => {}
        }
        if config.is_bot(&e.actor) {
            continue;
        }
        let by_author = config::same_login(&e.actor, &author);
        let by_maintainer = !by_author && config.is_maintainer(&e.actor);
        let waiting_label = e.label.as_deref() == Some(WAITING_LABEL);
        let blocks =
            by_maintainer && (e.kind == Kind::ReviewChangesRequested || (e.kind == Kind::LabelAdded && waiting_label));
        let unblocks = (by_author && matches!(e.kind, Kind::CommitPushed | Kind::Comment | Kind::ReviewRequested))
            || (e.kind == Kind::LabelRemoved && waiting_label);
        if blocks {
            if blocked_since.is_none() {
                blocked_since = Some(e.at);
            }
        } else if unblocks {
            if let Some(start) = blocked_since.take() {
                clock += e.at - start;
                reminded = false;
            }
        }
    }
    let author_is_blocker = blocked_since.is_some();
    if let Some(start) = blocked_since {
        clock += now - start;
    }

    let remind_at = i64::from(config.remind_days) * SECONDS_PER_DAY;
    let close_at = i64::from(config.close_days) * SECONDS_PER_DAY;
    let action = if !author_is_blocker {
        Action::None
    } else if clock >= close_at && !proposed {
        Action::ProposeClose
    } else if clock >= remind_at && !reminded {
        Action::Remind
    } else {
        Action::None
    };
    Ok(Evaluation { clock_seconds: clock, clock_days: clock / SECONDS_PER_DAY, action, author_is_blocker, author })
}

/// Parses the event file and config text (if any), `now`, and evaluates.
pub fn evaluate_inputs(events: &[u8], config: Option<&str>, now: &str) -> Result<Evaluation, TimelineError> {
    let events = parse_events(events)?;
    let config = match config {
        Some(text) => parse_config(text)?,
        None => Config::default(),
    };
    let now = parse_timestamp(now).map_err(|e| TimelineError::input(format!("--now: {}", e.message)))?;
    evaluate(&events, &config, now)
}
