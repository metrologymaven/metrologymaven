//! `cargo xtask plat-matrix lint-tiers [<file>]`: the tier definitions lint (spec node
//! `PLAT.MATRIX.POLICY.DEFS`).
//!
//! Finds the first Markdown table in the `## Tiers` section of `docs/platform/tiers.md` and
//! checks that it has the columns [`Column::ALL`], exactly one row each for Tier 1, Tier 2
//! and Tier 3, and no unfilled cell in those rows. Fenced code blocks are ignored, and the
//! section ends at the next level 1 or 2 heading (ATX or setext). The check is a pure
//! function of the text (D0): findings come in a fixed order (missing columns in column order, then per tier in tier
//! order: missing tier, or duplicate tier then unfilled cells in column order).

use std::fmt;

/// The shipped document, relative to the workspace root.
pub const DEFAULT_PATH: &str = "docs/platform/tiers.md";

/// The largest document the lint reads; bigger files are refused unread.
pub const MAX_BYTES: u64 = 1 << 20;

/// The heading whose section holds the table.
const SECTION: &str = "## Tiers";

/// A support tier, named as in `platform/support-matrix.toml`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Tier {
    Tier1,
    Tier2,
    Tier3,
}

impl Tier {
    pub const ALL: [Tier; 3] = [Tier::Tier1, Tier::Tier2, Tier::Tier3];

    /// The matrix-file name: `tier1`, `tier2`, `tier3`.
    pub fn name(self) -> &'static str {
        match self {
            Tier::Tier1 => "tier1",
            Tier::Tier2 => "tier2",
            Tier::Tier3 => "tier3",
        }
    }

    /// Reads a Tier-column cell: `Tier 1`, `tier1`, `**Tier 1**` (case, spaces and `*` ignored).
    fn from_cell(cell: &str) -> Option<Tier> {
        let key: String =
            cell.chars().filter(|c| !c.is_whitespace() && *c != '*').flat_map(char::to_lowercase).collect();
        Tier::ALL.into_iter().find(|t| t.name() == key)
    }
}

impl fmt::Display for Tier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// A required column of the tier table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Column {
    Tier,
    CiObligation,
    ReleaseBlocking,
    SupportResponse,
    PromotionDemotion,
}

impl Column {
    pub const ALL: [Column; 5] = [
        Column::Tier,
        Column::CiObligation,
        Column::ReleaseBlocking,
        Column::SupportResponse,
        Column::PromotionDemotion,
    ];

    /// The header text (matched case-insensitively after trimming).
    pub fn header(self) -> &'static str {
        match self {
            Column::Tier => "Tier",
            Column::CiObligation => "CI obligation",
            Column::ReleaseBlocking => "Release-blocking",
            Column::SupportResponse => "Support response",
            Column::PromotionDemotion => "Promotion/demotion",
        }
    }
}

impl fmt::Display for Column {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.header())
    }
}

/// A lint finding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TierError {
    /// No `## Tiers` heading, or no Markdown table in its section.
    NoTable,
    /// The table has no row for this tier.
    MissingTier { tier: Tier },
    /// The table has more than one row for this tier (the first one is checked).
    DuplicateTier { tier: Tier },
    /// The table header has no such column.
    MissingColumn { column: Column },
    /// The tier's row has an empty, absent or placeholder-only (`-`, `**`) cell in this column.
    EmptyCell { tier: Tier, column: Column },
}

impl fmt::Display for TierError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TierError::NoTable => write!(f, "NoTable: no Markdown table under `{SECTION}`"),
            TierError::MissingTier { tier } => write!(f, "MissingTier: {tier}"),
            TierError::DuplicateTier { tier } => write!(f, "DuplicateTier: {tier}"),
            TierError::MissingColumn { column } => write!(f, "MissingColumn: {column}"),
            TierError::EmptyCell { tier, column } => write!(f, "EmptyCell: {tier}, column {column}"),
        }
    }
}

/// Splits a table line into trimmed cells. Outer pipes are optional; `\|` stays in a cell.
fn cells(line: &str) -> Vec<String> {
    let line = line.trim();
    let line = line.strip_prefix('|').unwrap_or(line);
    let line = match line.strip_suffix('|') {
        Some(rest) if !rest.ends_with('\\') => rest,
        _ => line,
    };
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut escaped = false;
    for c in line.chars() {
        if escaped {
            cur.push(c);
            escaped = false;
        } else if c == '\\' {
            cur.push(c);
            escaped = true;
        } else if c == '|' {
            out.push(cur.trim().to_string());
            cur.clear();
        } else {
            cur.push(c);
        }
    }
    out.push(cur.trim().to_string());
    out
}

/// A delimiter row: every cell is dashes with optional alignment colons.
fn is_delimiter(line: &str) -> bool {
    line.contains('-')
        && cells(line).iter().all(|c| {
            let inner = c.strip_prefix(':').unwrap_or(c);
            let inner = inner.strip_suffix(':').unwrap_or(inner);
            !inner.is_empty() && inner.chars().all(|ch| ch == '-')
        })
}

fn is_table_line(line: &str) -> bool {
    line.trim_start().starts_with('|')
}

/// The fence marker (```` ``` ```` or `~~~`) a line opens or closes, if any.
fn fence_marker(line: &str) -> Option<char> {
    let t = line.trim_start();
    if t.starts_with("```") {
        Some('`')
    } else if t.starts_with("~~~") {
        Some('~')
    } else {
        None
    }
}

/// The document's lines with a flag per line: true when the line is outside fenced code
/// blocks (fence lines themselves count as inside). An unclosed fence runs to the end.
fn outside_fences(text: &str) -> Vec<(&str, bool)> {
    let mut open: Option<char> = None;
    text.lines()
        .map(|line| match (open, fence_marker(line)) {
            (None, Some(m)) => {
                open = Some(m);
                (line, false)
            }
            (Some(o), Some(m)) if o == m => {
                open = None;
                (line, false)
            }
            (o, _) => (line, o.is_none()),
        })
        .collect()
}

/// An ATX heading of level 1 or 2 (`# x`, `## x`, or a bare `#` / `##`).
fn is_atx_h1_h2(line: &str) -> bool {
    let t = line.trim_end();
    let indent = t.len() - t.trim_start().len();
    let t = t.trim_start();
    indent <= 3 && (t == "#" || t == "##" || t.starts_with("# ") || t.starts_with("## "))
}

/// A setext underline (`===` for level 1, `---` for level 2).
fn is_setext_underline(line: &str) -> bool {
    let t = line.trim();
    !t.is_empty() && (t.chars().all(|c| c == '=') || t.chars().all(|c| c == '-'))
}

/// Header and body rows of the first table in the `## Tiers` section. Lines inside fenced
/// code blocks are ignored. The section ends at the next level 1 or 2 heading, ATX or
/// setext.
fn find_table(text: &str) -> Option<(Vec<String>, Vec<Vec<String>>)> {
    let lines = outside_fences(text);
    let start = lines.iter().position(|(l, out)| *out && l.trim_end() == SECTION)? + 1;
    let live = |i: usize| lines.get(i).filter(|(_, out)| *out).map(|(l, _)| *l);
    for (i, &(line, out)) in lines.iter().enumerate().skip(start) {
        if !out {
            continue;
        }
        let next = live(i + 1);
        if is_atx_h1_h2(line) {
            return None;
        }
        if is_table_line(line) && next.is_some_and(|d| is_table_line(d) && is_delimiter(d)) {
            let body = (i + 2..).map_while(live).take_while(|l| is_table_line(l)).map(cells).collect();
            return Some((cells(line), body));
        }
        // A paragraph line followed by `===` or `---` is a setext heading.
        if !line.trim().is_empty() && next.is_some_and(is_setext_underline) {
            return None;
        }
    }
    None
}

/// A cell with no content: empty, or only dashes, emphasis marks, backticks and spaces.
fn is_unfilled(cell: &str) -> bool {
    cell.chars().all(|c| c.is_whitespace() || matches!(c, '-' | '*' | '_' | '`' | '\u{2013}' | '\u{2014}'))
}

/// Lints a tier definitions document. An empty result means the table is complete.
pub fn lint(text: &str) -> Vec<TierError> {
    let Some((header, body)) = find_table(text) else {
        return vec![TierError::NoTable];
    };
    let index = |col: Column| header.iter().position(|h| h.eq_ignore_ascii_case(col.header()));
    let mut errors: Vec<TierError> = Column::ALL
        .into_iter()
        .filter(|c| index(*c).is_none())
        .map(|column| TierError::MissingColumn { column })
        .collect();
    let Some(tier_col) = index(Column::Tier) else {
        return errors;
    };
    for tier in Tier::ALL {
        let rows: Vec<&Vec<String>> =
            body.iter().filter(|r| r.get(tier_col).and_then(|c| Tier::from_cell(c)) == Some(tier)).collect();
        let Some(row) = rows.first() else {
            errors.push(TierError::MissingTier { tier });
            continue;
        };
        if rows.len() > 1 {
            errors.push(TierError::DuplicateTier { tier });
        }
        for column in Column::ALL {
            if let Some(j) = index(column) {
                if !row.get(j).is_some_and(|c| !is_unfilled(c)) {
                    errors.push(TierError::EmptyCell { tier, column });
                }
            }
        }
    }
    errors
}
