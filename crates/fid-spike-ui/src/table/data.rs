//! The synthetic characteristic table of the leaf design: rows `{ id, balloon,
//! characteristic, nominal, upper, lower, actual, deviation, status, uec, unit, method,
//! feature, note }` with every number held as a decimal string, generated from SplitMix64
//! (seed 9) by the rules in `fixtures/viz/char-table-100k/make_table.py`.
//!
//! Every text in the table comes from the generator's fixed word lists or from an accepted
//! decimal, so no file data reaches the screen; [`Table::parse_tsv`] (for the committed
//! sample) accepts only text without control or format characters.

use super::dec::Dec;
use std::fmt;

/// Rows of the fixture table.
pub const FIXTURE_ROWS: u32 = 100_000;
/// Seed of the fixture table.
pub const FIXTURE_SEED: u64 = 9;
/// Largest table accepted by [`Table::generate`] and [`Table::parse_tsv`].
pub const MAX_ROWS: u32 = 1_000_000;
/// Largest TSV text accepted by [`Table::parse_tsv`].
pub const MAX_TSV_BYTES: usize = 128 * 1024 * 1024;
/// Longest text cell in bytes.
pub const MAX_TEXT_BYTES: usize = 256;

/// The 14 columns, in their initial display order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Column {
    /// Row id.
    Id,
    /// Balloon number.
    Balloon,
    /// Characteristic name.
    Characteristic,
    /// Nominal value.
    Nominal,
    /// Upper tolerance (a deviation limit).
    Upper,
    /// Lower tolerance (a deviation limit).
    Lower,
    /// Measured value, `-` when not measured.
    Actual,
    /// Actual minus nominal, `-` when not measured.
    Deviation,
    /// Status chip.
    Status,
    /// Expanded measurement uncertainty.
    Uec,
    /// Unit.
    Unit,
    /// Measuring method.
    Method,
    /// Feature.
    Feature,
    /// Free note.
    Note,
}

impl Column {
    /// All columns in initial display order.
    pub const ALL: [Column; 14] = [
        Column::Id,
        Column::Balloon,
        Column::Characteristic,
        Column::Nominal,
        Column::Upper,
        Column::Lower,
        Column::Actual,
        Column::Deviation,
        Column::Status,
        Column::Uec,
        Column::Unit,
        Column::Method,
        Column::Feature,
        Column::Note,
    ];

    /// Field name, as in the TSV header and the keyboard fixture.
    pub fn key(self) -> &'static str {
        match self {
            Column::Id => "id",
            Column::Balloon => "balloon",
            Column::Characteristic => "characteristic",
            Column::Nominal => "nominal",
            Column::Upper => "upper",
            Column::Lower => "lower",
            Column::Actual => "actual",
            Column::Deviation => "deviation",
            Column::Status => "status",
            Column::Uec => "uec",
            Column::Unit => "unit",
            Column::Method => "method",
            Column::Feature => "feature",
            Column::Note => "note",
        }
    }

    /// Column header title.
    pub fn title(self) -> &'static str {
        match self {
            Column::Id => "ID",
            Column::Balloon => "Balloon",
            Column::Characteristic => "Characteristic",
            Column::Nominal => "Nominal",
            Column::Upper => "Upper tol.",
            Column::Lower => "Lower tol.",
            Column::Actual => "Actual",
            Column::Deviation => "Deviation",
            Column::Status => "Status",
            Column::Uec => "UEC",
            Column::Unit => "Unit",
            Column::Method => "Method",
            Column::Feature => "Feature",
            Column::Note => "Note",
        }
    }

    /// Initial width in logical pixels.
    pub fn default_width(self) -> i32 {
        match self {
            Column::Id | Column::Balloon => 70,
            Column::Characteristic => 150,
            Column::Status => 120,
            Column::Uec => 80,
            Column::Unit => 50,
            Column::Method => 110,
            Column::Feature => 100,
            Column::Note => 160,
            _ => 90,
        }
    }

    /// Whether the cells hold decimals (compared exactly; `-` sorts after every number).
    pub fn numeric(self) -> bool {
        matches!(
            self,
            Column::Nominal | Column::Upper | Column::Lower | Column::Actual | Column::Deviation | Column::Uec
        )
    }

    /// Whether the cells can be edited (nominal and tolerances).
    pub fn editable(self) -> bool {
        matches!(self, Column::Nominal | Column::Upper | Column::Lower)
    }

    /// The column with this field name.
    pub fn from_key(s: &str) -> Option<Column> {
        Column::ALL.into_iter().find(|c| c.key() == s)
    }
}

/// Status of a characteristic, shown as a chip with an icon and a word.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Status {
    /// Within the inner band of the tolerance.
    Pass,
    /// Within tolerance, in the outer eighth of the band on either side.
    Warn,
    /// Outside tolerance.
    Fail,
    /// No actual value.
    NotMeasured,
}

impl Status {
    /// The word shown on the chip and announced to screen readers.
    pub fn word(self) -> &'static str {
        match self {
            Status::Pass => "Pass",
            Status::Warn => "Warn",
            Status::Fail => "Fail",
            Status::NotMeasured => "Not measured",
        }
    }

    /// The chip icon (drawn as a shape by the example, so it needs no icon font).
    pub fn icon(self) -> &'static str {
        match self {
            Status::Pass => "check",
            Status::Warn => "triangle",
            Status::Fail => "cross",
            Status::NotMeasured => "dash",
        }
    }

    fn parse(s: &str) -> Option<Status> {
        [Status::Pass, Status::Warn, Status::Fail, Status::NotMeasured].into_iter().find(|x| x.word() == s)
    }

    /// The status rule: unmeasured; above upper or below lower: fail;
    /// `|2 dev - upper - lower| * 4 > 3 (upper - lower)`: warn; else pass.
    pub fn of(dev: Option<Dec>, upper: Dec, lower: Dec) -> Status {
        let Some(d) = dev else { return Status::NotMeasured };
        let (d, u, l) = (d.scaled, upper.scaled, lower.scaled);
        if d > u || d < l {
            Status::Fail
        } else if (2 * d - u - l).abs() * 4 > 3 * (u - l) {
            Status::Warn
        } else {
            Status::Pass
        }
    }
}

/// One table row. Numeric cells are decimal strings kept exactly as generated or typed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// 1-based id.
    pub id: u32,
    /// Balloon number.
    pub balloon: u32,
    /// Characteristic name.
    pub characteristic: String,
    /// Nominal.
    pub nominal: String,
    /// Upper tolerance.
    pub upper: String,
    /// Lower tolerance.
    pub lower: String,
    /// Actual, `-` when not measured.
    pub actual: String,
    /// Deviation, `-` when not measured.
    pub deviation: String,
    /// Status.
    pub status: Status,
    /// Expanded uncertainty.
    pub uec: String,
    /// Unit.
    pub unit: String,
    /// Method.
    pub method: String,
    /// Feature.
    pub feature: String,
    /// Note (may be empty).
    pub note: String,
}

impl Row {
    /// The cell text of a column (what is shown, filtered, copied and announced).
    pub fn text(&self, c: Column) -> std::borrow::Cow<'_, str> {
        use std::borrow::Cow;
        match c {
            Column::Id => Cow::Owned(self.id.to_string()),
            Column::Balloon => Cow::Owned(self.balloon.to_string()),
            Column::Characteristic => Cow::Borrowed(&self.characteristic),
            Column::Nominal => Cow::Borrowed(&self.nominal),
            Column::Upper => Cow::Borrowed(&self.upper),
            Column::Lower => Cow::Borrowed(&self.lower),
            Column::Actual => Cow::Borrowed(&self.actual),
            Column::Deviation => Cow::Borrowed(&self.deviation),
            Column::Status => Cow::Borrowed(self.status.word()),
            Column::Uec => Cow::Borrowed(&self.uec),
            Column::Unit => Cow::Borrowed(&self.unit),
            Column::Method => Cow::Borrowed(&self.method),
            Column::Feature => Cow::Borrowed(&self.feature),
            Column::Note => Cow::Borrowed(&self.note),
        }
    }

    /// The row as one TSV line with a trailing newline.
    pub fn tsv_line(&self) -> String {
        let mut s = String::new();
        for (k, c) in Column::ALL.into_iter().enumerate() {
            if k > 0 {
                s.push('\t');
            }
            s.push_str(&self.text(c));
        }
        s.push('\n');
        s
    }

    /// Recomputes deviation and status from actual, nominal and the tolerances. Returns
    /// `false` (and changes nothing) if a numeric cell does not parse.
    pub fn recompute(&mut self) -> bool {
        let (Some(n), Some(u), Some(l)) = (Dec::parse(&self.nominal), Dec::parse(&self.upper), Dec::parse(&self.lower))
        else {
            return false;
        };
        if self.actual == "-" {
            self.deviation = "-".to_string();
            self.status = Status::NotMeasured;
            return true;
        }
        let Some(a) = Dec::parse(&self.actual) else { return false };
        let dev = a.minus(n);
        self.deviation = dev.format();
        self.status = Status::of(Some(dev), u, l);
        true
    }
}

/// The TSV header line (field names), with a trailing newline.
pub fn tsv_header() -> String {
    let keys: Vec<&str> = Column::ALL.iter().map(|c| c.key()).collect();
    format!("{}\n", keys.join("\t"))
}

/// FNV-1a 64 (used to compare the generated table with the Python reference's digest; not a
/// security hash).
pub fn fnv1a(data: &[u8]) -> u64 {
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    for b in data {
        h = (h ^ u64::from(*b)).wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// Why a table could not be built or parsed (1-based line numbers).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TableError {
    /// More than [`MAX_ROWS`] rows or [`MAX_TSV_BYTES`] bytes.
    TooLarge,
    /// The header line is not the 14 field names.
    Header,
    /// A line is malformed.
    Line(usize),
}

impl fmt::Display for TableError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TableError::TooLarge => write!(f, "table too large"),
            TableError::Header => write!(f, "table header is not the 14 field names"),
            TableError::Line(l) => write!(f, "table line {l}: malformed"),
        }
    }
}

impl std::error::Error for TableError {}

/// The table: rows in id order (`rows[i].id == i + 1` for a generated table).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Table {
    rows: Vec<Row>,
}

struct SplitMix64(u64);

impl SplitMix64 {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }

    fn pick<'a>(&mut self, list: &[&'a str]) -> &'a str {
        let k = self.below(list.len() as u64) as usize;
        list.get(k).copied().unwrap_or("")
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Class {
    Size,
    Angle,
    Form,
}

const CHARS: [(&str, &str, Class); 8] = [
    ("Diameter", "mm", Class::Size),
    ("Distance", "mm", Class::Size),
    ("Angle", "deg", Class::Angle),
    ("Position", "mm", Class::Form),
    ("Flatness", "mm", Class::Form),
    ("Profile", "mm", Class::Form),
    ("Roundness", "mm", Class::Form),
    ("Perpendicularity", "mm", Class::Form),
];
const FEATURES: [&str; 6] = ["Hole", "Plane", "Slot", "Cylinder", "Boss", "Edge"];
const METHODS: [&str; 4] = ["CMM tactile", "CMM scan", "Optical", "Gauge"];
const NOTES: [&str; 5] = ["datum A", "re-measured", "operator check", "first article", "see deviation report"];
const TOLS: [i64; 4] = [50, 100, 200, 500];

/// Formats a count of 0.0001 units with four decimal places.
fn fmt4(u: i64) -> String {
    let a = u.unsigned_abs();
    format!("{}{}.{:04}", if u < 0 { "-" } else { "" }, a / 10_000, a % 10_000)
}

fn status_units(dev: Option<i64>, upper: i64, lower: i64) -> Status {
    let Some(d) = dev else { return Status::NotMeasured };
    if d > upper || d < lower {
        Status::Fail
    } else if (2 * d - upper - lower).abs() * 4 > 3 * (upper - lower) {
        Status::Warn
    } else {
        Status::Pass
    }
}

fn plain(s: &str) -> bool {
    s.len() <= MAX_TEXT_BYTES && crate::rpt::canvas::plain_text(s)
}

impl Table {
    /// Generates `rows` rows from SplitMix64 seeded with `seed`, by the rules in
    /// `make_table.py` (draw order: characteristic, feature, nominal, tolerance, measured,
    /// deviation, uncertainty, method, note).
    pub fn generate(seed: u64, rows: u32) -> Result<Table, TableError> {
        if rows > MAX_ROWS {
            return Err(TableError::TooLarge);
        }
        let mut rng = SplitMix64(seed);
        let mut out = Vec::with_capacity(rows as usize);
        for id in 1..=rows {
            let (name, unit, class) = CHARS[rng.below(8) as usize % CHARS.len()];
            let feat = rng.pick(&FEATURES);
            let feature = format!("{feat} {}", 1 + rng.below(400));
            let d = rng.below(2_000_000) as i64;
            let nominal = match class {
                Class::Size => 10_000 + d,
                Class::Angle => d % 900_000,
                Class::Form => 0,
            };
            let t = TOLS[rng.below(4) as usize % TOLS.len()];
            let (upper, lower) = (t, if class == Class::Form { 0 } else { -t });
            let measured = rng.below(50) != 0;
            let span = t * 5 / 4;
            let e = rng.below((2 * span + 1) as u64) as i64;
            let dev = if class == Class::Form { e / 2 } else { e - span };
            let uec = 5 + rng.below(46) as i64;
            let method = rng.pick(&METHODS);
            let note = if rng.below(10) == 0 { rng.pick(&NOTES) } else { "" };
            out.push(Row {
                id,
                balloon: id,
                characteristic: name.to_string(),
                nominal: fmt4(nominal),
                upper: fmt4(upper),
                lower: fmt4(lower),
                actual: if measured { fmt4(nominal + dev) } else { "-".to_string() },
                deviation: if measured { fmt4(dev) } else { "-".to_string() },
                status: status_units(measured.then_some(dev), upper, lower),
                uec: fmt4(uec),
                unit: unit.to_string(),
                method: method.to_string(),
                feature,
                note: note.to_string(),
            });
        }
        Ok(Table { rows: out })
    }

    /// The fixture table: 10^5 rows, seed 9.
    pub fn fixture() -> Table {
        Table::generate(FIXTURE_SEED, FIXTURE_ROWS).unwrap_or(Table { rows: Vec::new() })
    }

    /// Parses TSV text (header line of field names, then rows). Ids must be distinct but
    /// need not be contiguous (the committed sample holds selected rows).
    pub fn parse_tsv(text: &str) -> Result<Vec<Row>, TableError> {
        if text.len() > MAX_TSV_BYTES {
            return Err(TableError::TooLarge);
        }
        let body = text.strip_suffix('\n').unwrap_or(text);
        let mut lines = body.split('\n');
        if lines.next().map(|h| format!("{h}\n")) != Some(tsv_header()) {
            return Err(TableError::Header);
        }
        let mut out: Vec<Row> = Vec::new();
        for (k, line) in lines.enumerate() {
            let ln = k + 2;
            if out.len() >= MAX_ROWS as usize {
                return Err(TableError::TooLarge);
            }
            let f: Vec<&str> = line.split('\t').collect();
            let row = (|| {
                let [id, balloon, ch, nom, up, lo, act, dev, st, uec, unit, method, feature, note] = f.as_slice()
                else {
                    return None;
                };
                let int = |s: &str| s.parse::<u32>().ok().filter(|v| v.to_string() == s);
                let dec = |s: &str| Dec::parse(s).map(|_| s.to_string());
                let opt = |s: &str| if s == "-" { Some(s.to_string()) } else { dec(s) };
                let txt = |s: &str| plain(s).then(|| s.to_string());
                Some(Row {
                    id: int(id)?,
                    balloon: int(balloon)?,
                    characteristic: txt(ch)?,
                    nominal: dec(nom)?,
                    upper: dec(up)?,
                    lower: dec(lo)?,
                    actual: opt(act)?,
                    deviation: opt(dev)?,
                    status: Status::parse(st)?,
                    uec: dec(uec)?,
                    unit: txt(unit)?,
                    method: txt(method)?,
                    feature: txt(feature)?,
                    note: txt(note)?,
                })
            })();
            let row = row.ok_or(TableError::Line(ln))?;
            if out.last().is_some_and(|p| p.id >= row.id) {
                return Err(TableError::Line(ln));
            }
            out.push(row);
        }
        Ok(out)
    }

    /// Number of rows.
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// Whether the table has no rows.
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// Rows in id order.
    pub fn rows(&self) -> &[Row] {
        &self.rows
    }

    /// The row with this id.
    pub fn get(&self, id: u32) -> Option<&Row> {
        let k = (id as usize).checked_sub(1)?;
        self.rows.get(k).filter(|r| r.id == id)
    }

    /// The row with this id, mutable.
    pub fn get_mut(&mut self, id: u32) -> Option<&mut Row> {
        let k = (id as usize).checked_sub(1)?;
        self.rows.get_mut(k).filter(|r| r.id == id)
    }

    /// The whole table as TSV (header, then one line per row).
    pub fn to_tsv(&self) -> String {
        let mut s = tsv_header();
        for r in &self.rows {
            s.push_str(&r.tsv_line());
        }
        s
    }

    /// Status counts (pass, warn, fail, not measured).
    pub fn status_counts(&self) -> [usize; 4] {
        let mut c = [0usize; 4];
        for r in &self.rows {
            let k = match r.status {
                Status::Pass => 0,
                Status::Warn => 1,
                Status::Fail => 2,
                Status::NotMeasured => 3,
            };
            if let Some(x) = c.get_mut(k) {
                *x += 1;
            }
        }
        c
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn hand_labelled_status_rule() {
        let d = |s: &str| Dec::parse(s).unwrap();
        let (u, l) = (d("0.0100"), d("-0.0100"));
        // Band 0.02 wide; the outer eighth on either side starts at |dev| > 0.0075.
        assert_eq!(Status::of(Some(d("0.0075")), u, l), Status::Pass);
        assert_eq!(Status::of(Some(d("0.0076")), u, l), Status::Warn);
        assert_eq!(Status::of(Some(d("-0.0100")), u, l), Status::Warn);
        assert_eq!(Status::of(Some(d("-0.0101")), u, l), Status::Fail);
        assert_eq!(Status::of(None, u, l), Status::NotMeasured);
        // Form tolerance 0 .. 0.01: the centre is 0.005.
        assert_eq!(Status::of(Some(d("0.0088")), u, d("0")), Status::Warn);
        assert_eq!(Status::of(Some(d("0.0087")), u, d("0")), Status::Pass);
        assert_eq!(fmt4(-260), "-0.0260");
        assert_eq!(fmt4(994_572), "99.4572");
        assert_eq!(fnv1a(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a(b"a"), 0xaf63_dc4c_8601_ec8c);
    }

    #[test]
    fn recompute_keeps_typed_text() {
        let mut t = Table::generate(9, 3).unwrap();
        let r = t.get_mut(2).unwrap();
        r.nominal = "25.1".to_string();
        assert!(r.recompute());
        assert_eq!((r.nominal.as_str(), r.deviation.as_str(), r.status), ("25.1", "74.3592", Status::Fail));
        r.upper = "x".to_string();
        assert!(!r.recompute());
    }
}
