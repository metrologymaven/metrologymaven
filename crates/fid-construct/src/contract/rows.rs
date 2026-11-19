//! The command data-row format of `construct.<verb_object>@1` registry commands
//! (REQ-KRN.CONS.DECL-006).
//!
//! The kernel may not depend on `CORE`, so construction commands are declared as data: a
//! [`CommandRow`] renders to one canonical JSON object (schema
//! `schemas/construct/command-row-1.schema.json`) that `CORE.REG` reads. Every row has side
//! effect `W2` (committing the constructed element) with dry run `R1` (the scratch result),
//! a reproducible determinism class, the algorithms the command may run, and its parameter
//! and result fields. The parameters start with [`COMMON_PARAMS`] (the input references and
//! the two general thresholds); the results start with [`JOURNAL_FIELDS`] (the journal
//! fields). Every numeric field (`number`, `integer`, `number_array`, `points`) declares a
//! quantity kind and its SI unit; no other field declares one.
//!
//! # Pinned JSON form
//!
//! RFC 8785 canonical JSON, keys sorted:
//!
//! ```text
//! {"algorithms":["<algorithm id>",...],"command":"<command id>","determinism":"<class>",
//!  "dry_run":"R1","params":[<field>,...],"result":[<field>,...],
//!  "row":"construct.command_row@1","side_effect":"W2"}
//! <field> = {"name":"<name>","required":true|false,"type":"<type>"
//!            [,"x-quantity":"<kind>","x-unit":"<unit>"]}
//! ```

use fid_types::{CommandId, Determinism, SideEffect};

use super::algorithm::ConstructAlgorithmId;
use super::json::{self, J};
use super::params::QuantityKind;

/// The row-format identifier written in every row.
pub const ROW_FORMAT: &str = "construct.command_row@1";

/// Longest field name, in bytes.
pub const MAX_FIELD_NAME: usize = 64;

/// A field's value type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueType {
    Number,
    Integer,
    NumberArray,
    /// A flat array of xyz triples.
    Points,
    String,
    Boolean,
    /// One element ID (canonical UUID text).
    ElementRef,
    /// An ordered array of element IDs.
    ElementRefArray,
}

impl ValueType {
    /// The wire string.
    pub const fn as_str(self) -> &'static str {
        match self {
            ValueType::Number => "number",
            ValueType::Integer => "integer",
            ValueType::NumberArray => "number_array",
            ValueType::Points => "points",
            ValueType::String => "string",
            ValueType::Boolean => "boolean",
            ValueType::ElementRef => "element_ref",
            ValueType::ElementRefArray => "element_ref_array",
        }
    }

    /// Whether values of this type are numbers (and so need a quantity kind and unit).
    pub const fn is_numeric(self) -> bool {
        matches!(self, ValueType::Number | ValueType::Integer | ValueType::NumberArray | ValueType::Points)
    }
}

/// One parameter or result field (plain data; checked when a row is built).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Field {
    /// `[a-z][a-z0-9_.]*`, at most [`MAX_FIELD_NAME`] bytes.
    pub name: &'static str,
    pub ty: ValueType,
    /// The quantity kind (its SI unit is implied); `Some` exactly for numeric types.
    pub quantity: Option<QuantityKind>,
    pub required: bool,
}

impl Field {
    /// A field.
    pub const fn new(name: &'static str, ty: ValueType, quantity: Option<QuantityKind>, required: bool) -> Field {
        Field { name, ty, quantity, required }
    }
}

/// Parameters every construction command takes first, in row order: the input element
/// references (declared order) and the two general thresholds of `ConstructParams`.
pub const COMMON_PARAMS: [Field; 3] = [
    Field::new("inputs", ValueType::ElementRefArray, None, true),
    Field::new("thresholds.angular", ValueType::Number, Some(QuantityKind::Angle), true),
    Field::new("thresholds.distance", ValueType::Number, Some(QuantityKind::Length), true),
];

/// Result fields every construction command returns first, in row order: the journal
/// fields of the `construct.*` event.
pub const JOURNAL_FIELDS: [Field; 4] = [
    Field::new("algorithm", ValueType::String, None, true),
    Field::new("inputs", ValueType::ElementRefArray, None, true),
    Field::new("params_digest", ValueType::String, None, true),
    Field::new("output_digest", ValueType::String, None, true),
];

/// Why a row is not valid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowError {
    /// A numeric field without a quantity kind.
    MissingQuantity { field: &'static str },
    /// A non-numeric field with a quantity kind.
    UnexpectedQuantity { field: &'static str },
    /// Two fields of one list share a name (including a clash with the common fields).
    DuplicateField { field: &'static str },
    /// A field name breaks the grammar.
    BadFieldName { field: &'static str },
    /// The command is not `construct.<verb_object>@<major>` with `<verb_object>` matching
    /// `[a-z][a-z0-9_]*`.
    Command,
    /// The row lists no algorithm, or its algorithms are not strictly ascending.
    Algorithms,
    /// The determinism class is `N` (observed); a kernel construction must be reproducible.
    Determinism,
}

impl RowError {
    /// A stable machine-readable code.
    pub const fn code(&self) -> &'static str {
        match self {
            RowError::MissingQuantity { .. } => "missing_quantity",
            RowError::UnexpectedQuantity { .. } => "unexpected_quantity",
            RowError::DuplicateField { .. } => "duplicate_field",
            RowError::BadFieldName { .. } => "bad_field_name",
            RowError::Command => "command",
            RowError::Algorithms => "algorithms",
            RowError::Determinism => "determinism",
        }
    }
}

/// Whether `c` is a construction command ID: name `construct.<verb_object>`, the second
/// segment `[a-z][a-z0-9_]*` with no further dot.
pub fn is_construct_command(c: &CommandId) -> bool {
    match c.name().strip_prefix("construct.") {
        Some(v) => {
            let mut b = v.bytes();
            matches!(b.next(), Some(x) if x.is_ascii_lowercase())
                && b.all(|x| x.is_ascii_lowercase() || x.is_ascii_digit() || x == b'_')
        }
        None => false,
    }
}

fn name_ok(s: &str) -> bool {
    let mut b = s.bytes();
    s.len() <= MAX_FIELD_NAME
        && matches!(b.next(), Some(c) if c.is_ascii_lowercase())
        && b.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_' || c == b'.')
}

/// Checks one field list: names well formed and unique, and a quantity kind on exactly the
/// numeric fields (first failing field in list order).
pub fn validate_fields(fields: &[Field]) -> Result<(), RowError> {
    for (i, f) in fields.iter().enumerate() {
        if !name_ok(f.name) {
            return Err(RowError::BadFieldName { field: f.name });
        }
        if fields[..i].iter().any(|g| g.name == f.name) {
            return Err(RowError::DuplicateField { field: f.name });
        }
        match (f.ty.is_numeric(), f.quantity) {
            (true, None) => return Err(RowError::MissingQuantity { field: f.name }),
            (false, Some(_)) => return Err(RowError::UnexpectedQuantity { field: f.name }),
            _ => {}
        }
    }
    Ok(())
}

/// One command declaration row. Fields are private; [`CommandRow::new`] checks everything.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandRow {
    command: CommandId,
    algorithms: Vec<ConstructAlgorithmId>,
    determinism: Determinism,
    params: Vec<Field>,
    result: Vec<Field>,
}

impl CommandRow {
    /// The row of `command`: [`COMMON_PARAMS`] then `params`, [`JOURNAL_FIELDS`] then
    /// `result` (the element fields, for example `element.c`). Checks, in order: the command
    /// ([`is_construct_command`]), the determinism class, the algorithms (non-empty, strictly
    /// ascending), then the parameter list and the result list ([`validate_fields`]).
    pub fn new(
        command: CommandId,
        algorithms: &[ConstructAlgorithmId],
        determinism: Determinism,
        params: &[Field],
        result: &[Field],
    ) -> Result<CommandRow, RowError> {
        if !is_construct_command(&command) {
            return Err(RowError::Command);
        }
        if determinism == Determinism::N {
            return Err(RowError::Determinism);
        }
        if algorithms.is_empty() || algorithms.windows(2).any(|w| w[0] >= w[1]) {
            return Err(RowError::Algorithms);
        }
        let params: Vec<Field> = COMMON_PARAMS.iter().chain(params).copied().collect();
        let result: Vec<Field> = JOURNAL_FIELDS.iter().chain(result).copied().collect();
        validate_fields(&params)?;
        validate_fields(&result)?;
        Ok(CommandRow { command, algorithms: algorithms.to_vec(), determinism, params, result })
    }

    /// The command ID.
    pub fn command(&self) -> &CommandId {
        &self.command
    }

    /// The algorithms the command may run, ascending.
    pub fn algorithms(&self) -> &[ConstructAlgorithmId] {
        &self.algorithms
    }

    /// Always `W2`: a commit writes the constructed element.
    pub fn side_effect(&self) -> SideEffect {
        SideEffect::W2
    }

    /// Always `R1`: a dry run computes the element as a scratch result.
    pub fn dry_run(&self) -> SideEffect {
        SideEffect::R1
    }

    /// The determinism class.
    pub fn determinism(&self) -> Determinism {
        self.determinism
    }

    /// The parameter fields, in row order.
    pub fn params(&self) -> &[Field] {
        &self.params
    }

    /// The result fields, in row order.
    pub fn result(&self) -> &[Field] {
        &self.result
    }

    /// The canonical JSON bytes of the row (module documentation).
    pub fn to_json(&self) -> Vec<u8> {
        json::write(&J::O(vec![
            ("algorithms", J::A(self.algorithms.iter().map(|a| J::S(a.to_string())).collect())),
            ("command", json::s(self.command.as_str())),
            ("determinism", json::s(self.determinism.as_str())),
            ("dry_run", json::s(self.dry_run().as_str())),
            ("params", fields_json(&self.params)),
            ("result", fields_json(&self.result)),
            ("row", json::s(ROW_FORMAT)),
            ("side_effect", json::s(self.side_effect().as_str())),
        ]))
    }
}

fn fields_json(fields: &[Field]) -> J {
    J::A(
        fields
            .iter()
            .map(|f| {
                let mut m =
                    vec![("name", json::s(f.name)), ("required", J::B(f.required)), ("type", json::s(f.ty.as_str()))];
                if let Some(q) = f.quantity {
                    m.push(("x-quantity", json::s(q.as_str())));
                    m.push(("x-unit", json::s(q.unit())));
                }
                J::O(m)
            })
            .collect(),
    )
}
