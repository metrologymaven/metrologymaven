-- Catalog schema v1 (spec node RPT.CATALOG.STORE.SCHEMA). Fixed text: every open compares the
-- file's sqlite_master with what this DDL produces, so a statement may never change once
-- released. Comments stand only between statements (SQLite stores a statement's own text).
-- Every text column that comes from project content is bounded in bytes and checked by shape
-- where it has one; it is stored as data and read back as untrusted text (CONSTITUTION C6).

CREATE TABLE catalog_meta (
    singleton INTEGER PRIMARY KEY NOT NULL CHECK (singleton = 1),
    schema_version INTEGER NOT NULL CHECK (schema_version >= 1),
    catalog_id TEXT NOT NULL CHECK (length(catalog_id) = 36 AND catalog_id NOT GLOB '*[^0-9a-f-]*'),
    owner TEXT CHECK (owner IS NULL OR length(CAST(owner AS BLOB)) BETWEEN 1 AND 256)
) STRICT;

CREATE TABLE project (
    project_id TEXT PRIMARY KEY NOT NULL CHECK (length(project_id) = 36 AND project_id NOT GLOB '*[^0-9a-f-]*'),
    handle TEXT NOT NULL UNIQUE COLLATE NOCASE CHECK (length(CAST(handle AS BLOB)) BETWEEN 1 AND 256),
    last_seq INTEGER CHECK (last_seq IS NULL OR last_seq >= 1),
    head_hash TEXT CHECK (head_hash IS NULL OR (length(head_hash) = 71 AND substr(head_hash, 1, 7) = 'sha256:' AND substr(head_hash, 8) NOT GLOB '*[^0-9a-f]*')),
    availability TEXT NOT NULL CHECK (availability IN ('available', 'unavailable')),
    CHECK ((last_seq IS NULL) = (head_hash IS NULL))
) STRICT;

CREATE TABLE entry (
    project_id TEXT NOT NULL REFERENCES project (project_id) ON DELETE CASCADE,
    record_id TEXT NOT NULL CHECK (length(CAST(record_id AS BLOB)) BETWEEN 1 AND 256),
    part TEXT NOT NULL CHECK (length(CAST(part AS BLOB)) BETWEEN 1 AND 1024),
    revision TEXT CHECK (revision IS NULL OR length(CAST(revision AS BLOB)) BETWEEN 1 AND 256),
    plan TEXT NOT NULL CHECK (length(CAST(plan AS BLOB)) BETWEEN 1 AND 1024),
    run TEXT NOT NULL CHECK (length(CAST(run AS BLOB)) BETWEEN 1 AND 256),
    stage TEXT NOT NULL CHECK (length(CAST(stage AS BLOB)) BETWEEN 1 AND 256),
    characteristic TEXT NOT NULL CHECK (length(CAST(characteristic AS BLOB)) BETWEEN 1 AND 1024),
    timestamp TEXT NOT NULL CHECK (length(timestamp) BETWEEN 20 AND 40 AND timestamp GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T[0-9][0-9]:[0-9][0-9]:[0-9][0-9]*Z' AND timestamp NOT GLOB '*[^0-9TZ:.-]*'),
    value_literal TEXT CHECK (value_literal IS NULL OR (length(value_literal) BETWEEN 1 AND 64 AND value_literal NOT GLOB '*[^0-9eE.+-]*')),
    unit TEXT CHECK (unit IS NULL OR length(CAST(unit AS BLOB)) BETWEEN 1 AND 64),
    si_value REAL CHECK (si_value IS NULL OR abs(si_value) <= 1.7976931348623157e308),
    decision_outcome TEXT NOT NULL CHECK (decision_outcome IN ('CONFORMS', 'NONCONFORMS', 'INDETERMINATE', 'NOT_DECIDABLE')),
    rule_id TEXT CHECK (rule_id IS NULL OR length(CAST(rule_id AS BLOB)) BETWEEN 1 AND 256),
    evaluation_basis TEXT NOT NULL CHECK (evaluation_basis IN ('POINTS', 'FEATURES', 'RESULTS_ONLY', 'DIRECT_READING')),
    uec_class TEXT NOT NULL CHECK (uec_class IN ('UEC-0', 'UEC-1', 'UEC-2', 'UEC-3')),
    claimed INTEGER NOT NULL CHECK (claimed IN (0, 1)),
    declared_input INTEGER NOT NULL CHECK (declared_input IN (0, 1)),
    uncharacterized_input INTEGER NOT NULL CHECK (uncharacterized_input IN (0, 1)),
    interpretation TEXT NOT NULL CHECK (interpretation IN ('provisional', 'signed_off')),
    record_status TEXT NOT NULL CHECK (record_status IN ('active', 'voided', 'superseded')),
    journal_seq INTEGER NOT NULL CHECK (journal_seq >= 1),
    journal_entry_hash TEXT NOT NULL CHECK (length(journal_entry_hash) = 71 AND substr(journal_entry_hash, 1, 7) = 'sha256:' AND substr(journal_entry_hash, 8) NOT GLOB '*[^0-9a-f]*'),
    entry_status TEXT NOT NULL CHECK (entry_status IN ('ok', 'unverified', 'stale', 'unavailable')),
    PRIMARY KEY (project_id, record_id),
    CHECK ((value_literal IS NULL) = (unit IS NULL) AND (value_literal IS NULL) = (si_value IS NULL)),
    CHECK (uec_class <> 'UEC-0' OR value_literal IS NULL),
    CHECK (claimed = 0 OR uec_class = 'UEC-1'),
    CHECK (uec_class NOT IN ('UEC-2', 'UEC-3') OR (declared_input = 0 AND uncharacterized_input = 0)),
    CHECK (uec_class <> 'UEC-0' OR (claimed = 0 AND declared_input = 0 AND uncharacterized_input = 0)),
    CHECK (uec_class <> 'UEC-1' OR claimed + uncharacterized_input = 1),
    CHECK (decision_outcome = 'NOT_DECIDABLE' OR rule_id IS NOT NULL)
) STRICT;

CREATE INDEX entry_by_part_characteristic ON entry (part, characteristic, timestamp, record_id);
