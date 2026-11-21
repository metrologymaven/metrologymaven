-- Catalog schema migration v1 -> v2 (spec node RPT.CATALOG.STORE.SCHEMA): entry tags and the
-- (project, journal position) index the incremental indexer reads. Fixed text, as v1.sql.

CREATE TABLE entry_tag (
    project_id TEXT NOT NULL,
    record_id TEXT NOT NULL,
    tag TEXT NOT NULL CHECK (length(CAST(tag AS BLOB)) BETWEEN 1 AND 128),
    PRIMARY KEY (project_id, record_id, tag),
    FOREIGN KEY (project_id, record_id) REFERENCES entry (project_id, record_id) ON DELETE CASCADE
) STRICT;

CREATE INDEX entry_by_project_position ON entry (project_id, journal_seq);
