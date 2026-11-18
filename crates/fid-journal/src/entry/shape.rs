//! Object shapes of `schema/entry.schema.json`: for every object in a header, its required
//! and optional keys, each property's schema (nested objects shown as `object`) and any
//! further keywords. The header checker reads its key lists from here, and the test
//! `schema_matches_rust_constants` compares every entry with the published schema, so
//! the two cannot drift apart unnoticed. Every shape has `additionalProperties: false`.

/// One object of the header schema.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ObjectShape {
    /// JSON pointer of the object's schema inside `entry.schema.json` (`""` = root).
    pub pointer: &'static str,
    pub required: &'static [&'static str],
    pub optional: &'static [&'static str],
    /// `(property, canonical JSON of its schema)`; a nested object's schema is `object`.
    pub props: &'static [(&'static str, &'static str)],
    /// Canonical JSON of the remaining keywords (not compared for the root).
    pub extra: &'static str,
}

/// Shape of the header itself.
pub const HEADER_SHAPE: ObjectShape = ObjectShape {
    pointer: "",
    required: &[
        "schema_version",
        "kind",
        "stream",
        "seq",
        "prev",
        "branch",
        "logical_parent",
        "time",
        "principal",
        "ai_assistance",
        "manifests",
        "hash",
    ],
    optional: &["reason_commit", "human_assertion", "command", "session", "upgrade", "head_move"],
    props: &[
        ("schema_version", r#"{"const":1}"#),
        ("kind", r#"{"enum":["command","session_header","principal_upgrade","head_move"]}"#),
        ("stream", r#"{"$ref":"#/$defs/ident"}"#),
        ("seq", r#"{"$ref":"#/$defs/seq"}"#),
        ("prev", r#"{"$ref":"#/$defs/digest"}"#),
        ("branch", r#"{"$ref":"#/$defs/branch"}"#),
        ("logical_parent", r#"{"anyOf":[{"type":"null"},{"$ref":"#/$defs/digest","not":{"const":"sha256:0000000000000000000000000000000000000000000000000000000000000000"}}]}"#),
        ("time", r#"object"#),
        ("principal", r#"object"#),
        ("ai_assistance", r#"{"enum":["none","suggested","authored","executed"]}"#),
        ("manifests", r#"object"#),
        ("reason_commit", r#"{"$ref":"#/$defs/digest"}"#),
        ("human_assertion", r#"{"$ref":"#/$defs/text"}"#),
        ("command", r#"object"#),
        ("session", r#"object"#),
        ("upgrade", r#"object"#),
        ("head_move", r#"object"#),
        ("hash", r#"{"$ref":"#/$defs/digest"}"#),
    ],
    extra: r#"{}"#,
};

/// Shape of `time`.
pub const TIME_SHAPE: ObjectShape = ObjectShape {
    pointer: "/properties/time",
    required: &["utc", "mono_ns", "sync"],
    optional: &[],
    props: &[
        ("utc", r#"{"$ref":"#/$defs/utc"}"#),
        ("mono_ns", r#"{"$ref":"#/$defs/u64"}"#),
        ("sync", r#"{"$ref":"#/$defs/text"}"#),
    ],
    extra: r#"{}"#,
};

/// Shape of `principal`.
pub const PRINCIPAL_SHAPE: ObjectShape = ObjectShape {
    pointer: "/properties/principal",
    required: &["kind", "commit"],
    optional: &["on_behalf_of_commit"],
    props: &[
        ("kind", r#"{"enum":["human","agent","script","service","device","system"]}"#),
        ("commit", r#"{"$ref":"#/$defs/digest"}"#),
        ("on_behalf_of_commit", r#"{"$ref":"#/$defs/digest"}"#),
    ],
    extra: r#"{}"#,
};

/// Shape of `manifests`.
pub const MANIFESTS_SHAPE: ObjectShape = ObjectShape {
    pointer: "/properties/manifests",
    required: &["registry", "kernel"],
    optional: &[],
    props: &[
        ("registry", r#"{"$ref":"#/$defs/digest"}"#),
        ("kernel", r#"{"$ref":"#/$defs/digest"}"#),
    ],
    extra: r#"{}"#,
};

/// Shape of the `command` body.
pub const COMMAND_SHAPE: ObjectShape = ObjectShape {
    pointer: "/properties/command",
    required: &[
        "id",
        "major",
        "class",
        "determinism",
        "seed",
        "args_commit",
        "events_commit",
        "inputs",
        "outputs",
        "algorithms",
    ],
    optional: &[],
    props: &[
        ("id", r#"{"$ref":"#/$defs/command_id"}"#),
        ("major", r#"{"maximum":4294967295,"minimum":0,"type":"integer"}"#),
        ("class", r#"{"enum":["R0","R1","W1","W2","W3","X1","I1","D1","M1","S1","A0"]}"#),
        ("determinism", r#"{"enum":["D0","D1","D2","D3","N"]}"#),
        ("seed", r#"{"$ref":"#/$defs/u64"}"#),
        ("args_commit", r#"{"$ref":"#/$defs/digest"}"#),
        ("events_commit", r#"{"$ref":"#/$defs/digest"}"#),
        ("inputs", r#"{"items":"object","type":"array"}"#),
        ("outputs", r#"{"items":"object","type":"array"}"#),
        ("algorithms", r#"{"items":{"$ref":"#/$defs/algorithm"},"type":"array"}"#),
    ],
    extra: r#"{}"#,
};

/// Shape of one tagged input.
pub const INPUT_SHAPE: ObjectShape = ObjectShape {
    pointer: "/properties/command/properties/inputs/items",
    required: &["tag", "digest"],
    optional: &[],
    props: &[
        ("tag", r#"{"$ref":"#/$defs/ident"}"#),
        ("digest", r#"{"$ref":"#/$defs/digest"}"#),
    ],
    extra: r#"{}"#,
};

/// Shape of one output.
pub const OUTPUT_SHAPE: ObjectShape = ObjectShape {
    pointer: "/properties/command/properties/outputs/items",
    required: &["digest"],
    optional: &["entity"],
    props: &[
        ("entity", r#"{"$ref":"#/$defs/text"}"#),
        ("digest", r#"{"$ref":"#/$defs/digest"}"#),
    ],
    extra: r#"{"if":{"not":{"required":["entity"]}},"then":{"properties":{"digest":{"not":{"const":"sha256:0000000000000000000000000000000000000000000000000000000000000000"}}}}}"#,
};

/// Shape of the `session` body.
pub const SESSION_SHAPE: ObjectShape = ObjectShape {
    pointer: "/properties/session",
    required: &["session_id", "app_version"],
    optional: &[],
    props: &[
        ("session_id", r#"{"$ref":"#/$defs/text"}"#),
        ("app_version", r#"{"$ref":"#/$defs/text"}"#),
    ],
    extra: r#"{}"#,
};

/// Shape of the `upgrade` body.
pub const UPGRADE_SHAPE: ObjectShape = ObjectShape {
    pointer: "/properties/upgrade",
    required: &["session_id", "method", "identity_commit", "confirmed_at"],
    optional: &[],
    props: &[
        ("session_id", r#"{"$ref":"#/$defs/text"}"#),
        ("method", r#"{"enum":["gui_prompt","os_presence"]}"#),
        ("identity_commit", r#"{"$ref":"#/$defs/digest"}"#),
        ("confirmed_at", r#"{"$ref":"#/$defs/utc"}"#),
    ],
    extra: r#"{}"#,
};

/// Shape of the `head_move` body.
pub const HEAD_MOVE_SHAPE: ObjectShape = ObjectShape {
    pointer: "/properties/head_move",
    required: &["from", "to"],
    optional: &[],
    props: &[
        ("from", r#"{"$ref":"#/$defs/digest"}"#),
        ("to", r#"{"$ref":"#/$defs/digest"}"#),
    ],
    extra: r#"{}"#,
};

/// Every object shape of the header schema.
pub const SHAPES: [ObjectShape; 10] = [
    HEADER_SHAPE,
    TIME_SHAPE,
    PRINCIPAL_SHAPE,
    MANIFESTS_SHAPE,
    COMMAND_SHAPE,
    INPUT_SHAPE,
    OUTPUT_SHAPE,
    SESSION_SHAPE,
    UPGRADE_SHAPE,
    HEAD_MOVE_SHAPE,
];
