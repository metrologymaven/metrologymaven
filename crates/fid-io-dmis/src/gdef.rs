//! Grammar family for definition and result statements (spec node `IO.DMIS.PARSE.GDEF`).
//!
//! Registers descriptors for the statements that DMIS output files and programs share:
//! header and bookkeeping (`DMISMN`, `DMISMD`, `FILNAM`, `ENDFIL`, `TEXT`), `UNITS`, feature
//! definitions `F(..)=FEAT/..` and actuals `FA(..)=FEAT/..`, tolerance definitions
//! `T(..)=TOL/..` and results `TA(..)=TOL/..`, and datum definitions `DATDEF`. Typed views
//! ([`view`]) expose labels, kinds and values as authored tokens; nothing is converted.
//!
//! The shapes are the project's own clean-room reading of how these statements are built,
//! written without copying any text or syntax table from the DMIS standard (ANSI/DMIS 105.2
//! Part 1, version 5.3; cited by designator only). They are deliberately **envelopes**: a
//! shape checks the kind word, the minor words that select a form (inner/outer, Cartesian or
//! polar) and a bounded count of values, not the meaning of each value. The kind lists and
//! the value counts are `[unverified]` against the standard, which is why every keyword is
//! declared `partial` for import in `conformance/declarations.toml`.
//!
//! Rules (project wording):
//!
//! - **Header**: `DMISMN` and `DMISMD` take a quoted name, a version number and up to 32
//!   further words or numbers; `FILNAM` a quoted name and an optional version; `ENDFIL`
//!   nothing; `TEXT` one word naming the channel and then the text, raw or quoted.
//! - **Units**: a length word (`MM`, `CM`, `METER`, `INCH`, `FEET`), an angle word (`ANGDEC`,
//!   `ANGDMS`, `ANGRAD`) and optionally a temperature word (`TEMPC`, `TEMPF`).
//! - **Geometric features** ([`FEATURES`]): the kind word; `INNER` or `OUTER` for kinds that
//!   have a material side; an optional form word for lines (`BND`, `UNBND`) and slots
//!   (`ROUND`, `SQUARE`, `FLAT`); `CART` or `POL`; then between `min` and `max` numbers.
//! - **Member features** (`PATERN`, `COMPOUND`): the kind word, for `COMPOUND` an optional
//!   word, then one or more feature labels (`F(..)` or `FA(..)`).
//! - **Tolerances** ([`TOLERANCES`]): the kind word, then 1 to [`TOL_TAIL_MAX`] fields that
//!   are each a number, a word or a datum label `DAT(..)`. A result (`TA`) has the same body
//!   with one field fewer allowed, followed by the flag `INTOL` or `OUTOL` as its last field.
//! - **Datum definition**: `DATDEF/` a feature label (`F` or `FA`), then a datum label (`DAT`
//!   or `DA`). No label prefix on the statement.
//! - Definitions take label prefixes `F` and `T`, actuals and results `FA` and `TA`; each is
//!   its own statement class (`feat_circle`, `feat_circle_actual`, ...).
//!
//! **Step budget.** Every repeat in these shapes is followed either by the end of the
//! statement or by a field of a kind the repeat cannot match, and every alternative lists
//! distinct keywords or token kinds, so a failing path is abandoned at the first field that
//! cannot match and backtracking stays linear in the field count. The only overlap is a
//! tolerance result's last field: a body word can also be `INTOL`/`OUTOL`, which costs at most
//! [`TOL_TAIL_MAX`] retries. Measured costs are in the `gdef` tests
//! (`realistic_statements_stay_well_under_the_step_cap`).
//!
//! **Not covered yet**: QPIds (REQ-IO.DMIS.PARSE.GDEF-004; the statement forms that carry them
//! have not been confirmed), conformance module tags, and the NIST test-suite corpus
//! (AC-3; no `corpus/dmis.toml` exists yet).

use crate::lex::{Token, TokenKind};
use crate::registry::{LabelKind, Shape, StatementDescriptor};
use crate::tree::{Label, Statement};

/// The spec node of this family, recorded on every descriptor.
pub const FAMILY: &str = "IO.DMIS.PARSE.GDEF";
/// The phase of this family.
pub const PHASE: &str = "P1a";
/// Most fields after the kind word in a tolerance definition.
pub const TOL_TAIL_MAX: u32 = 12;

/// Feature kinds of this family.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FeatKind {
    Point,
    Line,
    Plane,
    Circle,
    Arc,
    Cylinder,
    Cone,
    Sphere,
    Ellipse,
    Slot,
    Torus,
    EdgePoint,
    GenericCurve,
    GenericSurface,
    Pattern,
    Compound,
}

/// How a feature kind's fields are laid out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeatForm {
    /// Geometry given by numbers.
    Geometric {
        /// `INNER` or `OUTER` is required.
        side: bool,
        /// Optional form words before the coordinate word.
        form_words: &'static [&'static str],
        /// Fewest numbers.
        min: u32,
        /// Most numbers.
        max: u32,
    },
    /// A list of feature labels.
    Members {
        /// An optional word may come before the labels.
        lead_word: bool,
    },
}

/// One feature kind: its class stem, kind word and field layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FeatSpec {
    /// The kind.
    pub kind: FeatKind,
    /// Class stem: classes are `feat_<id>` and `feat_<id>_actual`.
    pub id: &'static str,
    /// The kind word (upper case).
    pub word: &'static str,
    /// Field layout.
    pub form: FeatForm,
}

const fn geo(kind: FeatKind, id: &'static str, word: &'static str, side: bool, min: u32, max: u32) -> FeatSpec {
    FeatSpec { kind, id, word, form: FeatForm::Geometric { side, form_words: &[], min, max } }
}

/// The feature kinds and their envelopes [unverified: kind list and value counts].
pub const FEATURES: &[FeatSpec] = &[
    geo(FeatKind::Point, "point", "POINT", false, 3, 6),
    FeatSpec {
        kind: FeatKind::Line,
        id: "line",
        word: "LINE",
        form: FeatForm::Geometric { side: false, form_words: &["BND", "UNBND"], min: 6, max: 12 },
    },
    geo(FeatKind::Plane, "plane", "PLANE", false, 6, 6),
    geo(FeatKind::Circle, "circle", "CIRCLE", true, 7, 7),
    geo(FeatKind::Arc, "arc", "ARC", true, 7, 10),
    geo(FeatKind::Cylinder, "cylndr", "CYLNDR", true, 7, 8),
    geo(FeatKind::Cone, "cone", "CONE", true, 7, 8),
    geo(FeatKind::Sphere, "sphere", "SPHERE", true, 4, 7),
    geo(FeatKind::Ellipse, "ellips", "ELLIPS", true, 8, 13),
    FeatSpec {
        kind: FeatKind::Slot,
        id: "cparln",
        word: "CPARLN",
        form: FeatForm::Geometric { side: true, form_words: &["ROUND", "SQUARE", "FLAT"], min: 8, max: 13 },
    },
    geo(FeatKind::Torus, "torus", "TORUS", true, 8, 8),
    geo(FeatKind::EdgePoint, "edgept", "EDGEPT", false, 6, 9),
    geo(FeatKind::GenericCurve, "gcurve", "GCURVE", false, 3, 9),
    geo(FeatKind::GenericSurface, "gsurf", "GSURF", false, 3, 9),
    FeatSpec { kind: FeatKind::Pattern, id: "patern", word: "PATERN", form: FeatForm::Members { lead_word: false } },
    FeatSpec {
        kind: FeatKind::Compound,
        id: "compound",
        word: "COMPOUND",
        form: FeatForm::Members { lead_word: true },
    },
];

/// Tolerance categories.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TolCategory {
    Size,
    Location,
    Form,
    Orientation,
    Profile,
    Runout,
}

impl TolCategory {
    /// Stable snake_case name.
    pub fn as_str(self) -> &'static str {
        match self {
            TolCategory::Size => "size",
            TolCategory::Location => "location",
            TolCategory::Form => "form",
            TolCategory::Orientation => "orientation",
            TolCategory::Profile => "profile",
            TolCategory::Runout => "runout",
        }
    }
}

/// One tolerance kind: its kind word (also the class stem in lower case) and category.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TolSpec {
    /// The kind word (upper case).
    pub word: &'static str,
    /// The category.
    pub category: TolCategory,
}

const fn tol(word: &'static str, category: TolCategory) -> TolSpec {
    TolSpec { word, category }
}

/// The tolerance kinds [unverified: kind list].
pub const TOLERANCES: &[TolSpec] = &[
    tol("DIAM", TolCategory::Size),
    tol("RAD", TolCategory::Size),
    tol("WIDTH", TolCategory::Size),
    tol("DISTB", TolCategory::Location),
    tol("ANGLB", TolCategory::Location),
    tol("POS", TolCategory::Location),
    tol("CONCEN", TolCategory::Location),
    tol("FLAT", TolCategory::Form),
    tol("STRGHT", TolCategory::Form),
    tol("CIRLTY", TolCategory::Form),
    tol("CYLCTY", TolCategory::Form),
    tol("PARLEL", TolCategory::Orientation),
    tol("PERPND", TolCategory::Orientation),
    tol("ANGLR", TolCategory::Orientation),
    tol("PROFL", TolCategory::Profile),
    tol("PROFP", TolCategory::Profile),
    tol("PROFS", TolCategory::Profile),
    tol("CRNOUT", TolCategory::Runout),
    tol("TRNOUT", TolCategory::Runout),
];

const ACTUAL_SUFFIX: &str = "_actual";

fn kws(words: &[&str]) -> Shape {
    Shape::alt(words.iter().map(|w| Shape::kw(w)).collect())
}

fn req(prefix: &str) -> LabelKind {
    LabelKind::Required(prefix.into())
}

fn desc(class: &str, major: &str, label: LabelKind, shape: Shape) -> StatementDescriptor {
    StatementDescriptor::new(class, major, label, shape, FAMILY, PHASE)
}

fn feat_shape(spec: &FeatSpec) -> Shape {
    let feature_label = || Shape::alt(vec![Shape::label("F"), Shape::label("FA")]);
    let mut items = vec![Shape::kw(spec.word)];
    match spec.form {
        FeatForm::Geometric { side, form_words, min, max } => {
            if side {
                items.push(kws(&["INNER", "OUTER"]));
            }
            if !form_words.is_empty() {
                items.push(Shape::opt(kws(form_words)));
            }
            items.push(kws(&["CART", "POL"]));
            items.push(Shape::repeat(Shape::Number, min, Some(max)));
        }
        FeatForm::Members { lead_word } => {
            if lead_word {
                items.push(Shape::opt(Shape::AnyWord));
            }
            items.push(Shape::repeat(feature_label(), 1, None));
        }
    }
    Shape::seq(items)
}

fn tol_item() -> Shape {
    Shape::alt(vec![Shape::Number, Shape::AnyWord, Shape::label("DAT")])
}

/// The descriptors of this family, in registration order.
pub fn descriptors() -> Vec<StatementDescriptor> {
    let none = || LabelKind::None;
    let header = || {
        Shape::seq(vec![
            Shape::Str,
            Shape::Number,
            Shape::repeat(Shape::alt(vec![Shape::AnyWord, Shape::Number]), 0, Some(32)),
        ])
    };
    let mut out = vec![
        desc("dmismn", "DMISMN", none(), header()),
        desc("dmismd", "DMISMD", none(), header()),
        desc("filnam", "FILNAM", none(), Shape::seq(vec![Shape::Str, Shape::opt(Shape::Number)])),
        desc("endfil", "ENDFIL", none(), Shape::seq(vec![])),
        desc("text", "TEXT", none(), Shape::seq(vec![Shape::AnyWord, Shape::alt(vec![Shape::RawText, Shape::Str])])),
        desc(
            "units",
            "UNITS",
            none(),
            Shape::seq(vec![
                kws(&["MM", "CM", "METER", "INCH", "FEET"]),
                kws(&["ANGDEC", "ANGDMS", "ANGRAD"]),
                Shape::opt(kws(&["TEMPC", "TEMPF"])),
            ]),
        ),
        desc(
            "datdef",
            "DATDEF",
            none(),
            Shape::seq(vec![
                Shape::alt(vec![Shape::label("F"), Shape::label("FA")]),
                Shape::alt(vec![Shape::label("DAT"), Shape::label("DA")]),
            ]),
        ),
    ];
    for spec in FEATURES {
        out.push(desc(&format!("feat_{}", spec.id), "FEAT", req("F"), feat_shape(spec)));
        out.push(desc(&format!("feat_{}{ACTUAL_SUFFIX}", spec.id), "FEAT", req("FA"), feat_shape(spec)));
    }
    for spec in TOLERANCES {
        let stem = spec.word.to_ascii_lowercase();
        let def = Shape::seq(vec![Shape::kw(spec.word), Shape::repeat(tol_item(), 1, Some(TOL_TAIL_MAX))]);
        let result = Shape::seq(vec![
            Shape::kw(spec.word),
            Shape::repeat(tol_item(), 1, Some(TOL_TAIL_MAX - 1)),
            kws(&["INTOL", "OUTOL"]),
        ]);
        out.push(desc(&format!("tol_{stem}"), "TOL", req("T"), def));
        out.push(desc(&format!("tol_{stem}{ACTUAL_SUFFIX}"), "TOL", req("TA"), result));
    }
    out
}

/// Cartesian or polar values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Coord {
    Cart,
    Pol,
}

/// Material side of a feature.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Side {
    Inner,
    Outer,
}

/// The in- or out-of-tolerance flag of a tolerance result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TolFlag {
    InTol,
    OutTol,
}

/// A header or bookkeeping statement (`DMISMN`, `DMISMD`, `FILNAM`, `ENDFIL`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeaderView<'a> {
    /// The quoted name, if any.
    pub name: Option<&'a Token>,
    /// The version number, if any.
    pub version: Option<&'a Token>,
    /// Further words and numbers, as authored.
    pub extra: Vec<&'a Token>,
}

/// A `TEXT` statement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextView<'a> {
    /// The channel word.
    pub channel: &'a Token,
    /// The text: a raw-text or string token (untrusted).
    pub text: &'a Token,
}

/// A `UNITS` statement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnitsView<'a> {
    /// The length unit word.
    pub length: &'a Token,
    /// The angle unit word.
    pub angle: &'a Token,
    /// The temperature unit word, if given.
    pub temp: Option<&'a Token>,
}

/// A feature definition or actual.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeatView<'a> {
    /// The `F(..)` or `FA(..)` label.
    pub label: &'a Label,
    /// True for an actual (`FA`).
    pub actual: bool,
    /// The kind.
    pub kind: FeatKind,
    /// The kind word as authored.
    pub kind_word: &'a Token,
    /// `CART` or `POL`, for geometric kinds.
    pub coord: Option<Coord>,
    /// `INNER` or `OUTER`, where the kind has a side.
    pub side: Option<Side>,
    /// Other minor words (line and slot forms, the compound word), as authored.
    pub minor: Vec<&'a Token>,
    /// The numbers, as authored lexemes.
    pub values: Vec<&'a Token>,
    /// Feature labels of a pattern or compound.
    pub refs: Vec<&'a Token>,
}

/// A tolerance definition or result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TolView<'a> {
    /// The `T(..)` or `TA(..)` label.
    pub label: &'a Label,
    /// True for a result (`TA`).
    pub actual: bool,
    /// The kind word as authored.
    pub kind_word: &'a Token,
    /// The kind's table entry.
    pub kind: TolSpec,
    /// The numbers, as authored lexemes.
    pub values: Vec<&'a Token>,
    /// Words of the body (modifiers), as authored; the result flag is not included.
    pub words: Vec<&'a Token>,
    /// Datum labels.
    pub refs: Vec<&'a Token>,
    /// `INTOL` or `OUTOL` on a result.
    pub flag: Option<TolFlag>,
}

/// A `DATDEF` statement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DatumView<'a> {
    /// The feature label.
    pub feature: &'a Token,
    /// The datum label.
    pub datum: &'a Token,
}

/// The typed view of a statement of this family.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GdefView<'a> {
    Header(HeaderView<'a>),
    Text(TextView<'a>),
    Units(UnitsView<'a>),
    Feat(FeatView<'a>),
    Tol(TolView<'a>),
    Datum(DatumView<'a>),
}

fn atoms(s: &Statement) -> Vec<&Token> {
    s.args.iter().filter_map(|a| a.atom()).collect()
}

fn upper(t: &Token) -> Option<&str> {
    match &t.kind {
        TokenKind::Word { upper } => Some(upper),
        _ => None,
    }
}

/// The typed view of `s`, if its class belongs to this family. Views read the statement's
/// tokens; they never convert numbers.
pub fn view(s: &Statement) -> Option<GdefView<'_>> {
    let class = &*s.class;
    match class {
        "dmismn" | "dmismd" | "filnam" | "endfil" => {
            let a = atoms(s);
            let name = a.first().copied().filter(|t| matches!(t.kind, TokenKind::Str { .. }));
            let version = a.get(1).copied().filter(|t| t.kind == TokenKind::Number);
            Some(GdefView::Header(HeaderView { name, version, extra: a.get(2..).unwrap_or(&[]).to_vec() }))
        }
        "text" => match *atoms(s).as_slice() {
            [channel, text] => Some(GdefView::Text(TextView { channel, text })),
            _ => None,
        },
        "units" => match *atoms(s).as_slice() {
            [length, angle] => Some(GdefView::Units(UnitsView { length, angle, temp: None })),
            [length, angle, temp] => Some(GdefView::Units(UnitsView { length, angle, temp: Some(temp) })),
            _ => None,
        },
        "datdef" => match *atoms(s).as_slice() {
            [feature, datum] => Some(GdefView::Datum(DatumView { feature, datum })),
            _ => None,
        },
        _ => {
            let (stem, actual) = match class.strip_suffix(ACTUAL_SUFFIX) {
                Some(stem) => (stem, true),
                None => (class, false),
            };
            if let Some(id) = stem.strip_prefix("feat_") {
                let spec = FEATURES.iter().find(|f| f.id == id)?;
                feat_view(s, spec, actual).map(GdefView::Feat)
            } else if let Some(id) = stem.strip_prefix("tol_") {
                let spec = TOLERANCES.iter().find(|t| t.word.eq_ignore_ascii_case(id))?;
                tol_view(s, *spec, actual).map(GdefView::Tol)
            } else {
                None
            }
        }
    }
}

fn feat_view<'a>(s: &'a Statement, spec: &FeatSpec, actual: bool) -> Option<FeatView<'a>> {
    let label = s.label.as_ref()?;
    let a = atoms(s);
    let (&kind_word, rest) = a.split_first()?;
    let mut v = FeatView {
        label,
        actual,
        kind: spec.kind,
        kind_word,
        coord: None,
        side: None,
        minor: Vec::new(),
        values: Vec::new(),
        refs: Vec::new(),
    };
    let geometric = matches!(spec.form, FeatForm::Geometric { .. });
    for &t in rest {
        match (&t.kind, upper(t)) {
            (_, Some("CART")) if geometric => v.coord = Some(Coord::Cart),
            (_, Some("POL")) if geometric => v.coord = Some(Coord::Pol),
            (_, Some("INNER")) if geometric => v.side = Some(Side::Inner),
            (_, Some("OUTER")) if geometric => v.side = Some(Side::Outer),
            (_, Some(_)) => v.minor.push(t),
            (TokenKind::Number, _) => v.values.push(t),
            (TokenKind::Label { .. }, _) => v.refs.push(t),
            _ => return None,
        }
    }
    Some(v)
}

fn tol_view(s: &Statement, kind: TolSpec, actual: bool) -> Option<TolView<'_>> {
    let label = s.label.as_ref()?;
    let a = atoms(s);
    let (&kind_word, mut rest) = a.split_first()?;
    let mut flag = None;
    if actual {
        let (last, body) = rest.split_last()?;
        flag = match upper(last) {
            Some("INTOL") => Some(TolFlag::InTol),
            Some("OUTOL") => Some(TolFlag::OutTol),
            _ => return None,
        };
        rest = body;
    }
    let mut v =
        TolView { label, actual, kind_word, kind, values: Vec::new(), words: Vec::new(), refs: Vec::new(), flag };
    for &t in rest {
        match t.kind {
            TokenKind::Number => v.values.push(t),
            TokenKind::Word { .. } => v.words.push(t),
            TokenKind::Label { .. } => v.refs.push(t),
            _ => return None,
        }
    }
    Some(v)
}
