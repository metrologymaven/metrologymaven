//! `cargo xtask plat-matrix lint-tiers [<file>]`: the tier definitions lint (spec node
//! `PLAT.MATRIX.POLICY.DEFS`).
//!
//! Finds the first Markdown table in the `## Tiers` section of `docs/platform/tiers.md` and
//! checks that it has the columns [`Column::ALL`], one row each for Tier 1, Tier 2 and
//! Tier 3, and no empty cell in those rows. The check is a pure function of the text (D0):
//! findings come in a fixed order (missing columns in column order, then per tier in tier
//! order: missing tier, or empty cells in column order).

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
    /// The table header has no such column.
    MissingColumn { column: Column },
    /// The tier's row has an empty (or absent) cell in this column.
    EmptyCell { tier: Tier, column: Column },
}

impl fmt::Display for TierError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TierError::NoTable => write!(f, "NoTable: no Markdown table under `{SECTION}`"),
            TierError::MissingTier { tier } => write!(f, "MissingTier: {tier}"),
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

/// Header and body rows of the first table in the `## Tiers` section.
fn find_table(text: &str) -> Option<(Vec<String>, Vec<Vec<String>>)> {
    let lines: Vec<&str> = text.lines().collect();
    let start = lines.iter().position(|l| l.trim_end() == SECTION)? + 1;
    for (i, line) in lines.iter().enumerate().skip(start) {
        // The section ends at the next heading of the same or a higher level.
        if line.starts_with("# ") || line.starts_with("## ") {
            return None;
        }
        if is_table_line(line) && lines.get(i + 1).is_some_and(|d| is_table_line(d) && is_delimiter(d)) {
            let rest = lines.get(i + 2..).unwrap_or(&[]);
            let body = rest.iter().take_while(|l| is_table_line(l)).map(|l| cells(l)).collect();
            return Some((cells(line), body));
        }
    }
    None
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
        let row = body.iter().find(|r| r.get(tier_col).and_then(|c| Tier::from_cell(c)) == Some(tier));
        let Some(row) = row else {
            errors.push(TierError::MissingTier { tier });
            continue;
        };
        for column in Column::ALL {
            if let Some(j) = index(column) {
                if !row.get(j).is_some_and(|c| !c.is_empty()) {
                    errors.push(TierError::EmptyCell { tier, column });
                }
            }
        }
    }
    errors
}
