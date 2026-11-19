-- fid-store project.db schema, version 1 (spec node CORE.STO.SQL.SCHEMA).
--
-- Applied by the migration runner as migration 0 -> 1 inside one BEGIN IMMEDIATE transaction;
-- the runner records the version in schema_meta. Also read by
-- fixtures/core/sto/sql/schema/make_fixture.py. No transaction control statements here.
--
-- Append-only tables (entries, entry_blobs, snapshots, checkpoints, anchors) and schema_meta
-- carry <t>_no_update and <t>_no_delete triggers raising 'append_only', plus <t>_no_replace,
-- which refuses an INSERT whose key already exists: INSERT OR REPLACE deletes the old row
-- without firing delete triggers unless recursive_triggers is on, which a raw connection
-- need not set. heads, idempotency and settings_current are the only mutable tables and can
-- be rebuilt from entries.
--
-- Digest columns hold 'sha256:' followed by 64 lowercase hex digits.

CREATE TABLE schema_meta (
    version INTEGER PRIMARY KEY NOT NULL CHECK (version >= 1)
);

CREATE TABLE entries (
    seq INTEGER PRIMARY KEY CHECK (seq >= 1),
    hash TEXT NOT NULL UNIQUE
        CHECK (length(hash) = 71 AND substr(hash, 1, 7) = 'sha256:' AND NOT substr(hash, 8) GLOB '*[^0-9a-f]*'),
    stream TEXT NOT NULL,
    branch TEXT NOT NULL,
    kind TEXT NOT NULL,
    prev_hash TEXT
        CHECK (prev_hash IS NULL OR (length(prev_hash) = 71 AND substr(prev_hash, 1, 7) = 'sha256:'
            AND NOT substr(prev_hash, 8) GLOB '*[^0-9a-f]*')),
    header BLOB NOT NULL
);
CREATE INDEX entries_by_stream ON entries (stream, seq);
CREATE INDEX entries_by_branch ON entries (branch, seq);

CREATE TABLE entry_blobs (
    entry_hash TEXT NOT NULL REFERENCES entries (hash),
    role TEXT NOT NULL,
    digest TEXT NOT NULL
        CHECK (length(digest) = 71 AND substr(digest, 1, 7) = 'sha256:' AND NOT substr(digest, 8) GLOB '*[^0-9a-f]*'),
    PRIMARY KEY (entry_hash, role, digest)
) WITHOUT ROWID;
CREATE INDEX entry_blobs_by_digest ON entry_blobs (digest);

CREATE TABLE heads (
    branch TEXT PRIMARY KEY NOT NULL,
    entry_hash TEXT NOT NULL REFERENCES entries (hash)
) WITHOUT ROWID;

CREATE TABLE snapshots (
    id INTEGER PRIMARY KEY,
    entry_hash TEXT NOT NULL REFERENCES entries (hash),
    format_version INTEGER NOT NULL CHECK (format_version >= 0),
    state_digest TEXT NOT NULL
        CHECK (length(state_digest) = 71 AND substr(state_digest, 1, 7) = 'sha256:'
            AND NOT substr(state_digest, 8) GLOB '*[^0-9a-f]*'),
    content_digest TEXT NOT NULL
        CHECK (length(content_digest) = 71 AND substr(content_digest, 1, 7) = 'sha256:'
            AND NOT substr(content_digest, 8) GLOB '*[^0-9a-f]*'),
    size INTEGER NOT NULL CHECK (size >= 0)
);
CREATE INDEX snapshots_by_entry ON snapshots (entry_hash);

CREATE TABLE checkpoints (
    id INTEGER PRIMARY KEY,
    stream TEXT NOT NULL,
    entry_hash TEXT NOT NULL REFERENCES entries (hash),
    body BLOB NOT NULL
);
CREATE INDEX checkpoints_by_stream ON checkpoints (stream, id);

CREATE TABLE anchors (
    id INTEGER PRIMARY KEY,
    checkpoint_id INTEGER NOT NULL REFERENCES checkpoints (id),
    kind TEXT NOT NULL,
    body BLOB NOT NULL
);
CREATE INDEX anchors_by_checkpoint ON anchors (checkpoint_id);

CREATE TABLE idempotency (
    key TEXT PRIMARY KEY NOT NULL,
    entry_hash TEXT NOT NULL REFERENCES entries (hash)
) WITHOUT ROWID;

CREATE TABLE settings_current (
    key TEXT PRIMARY KEY NOT NULL,
    value BLOB NOT NULL,
    entry_hash TEXT NOT NULL REFERENCES entries (hash)
) WITHOUT ROWID;

CREATE TRIGGER schema_meta_no_update BEFORE UPDATE ON schema_meta BEGIN SELECT RAISE(ABORT, 'append_only'); END;
CREATE TRIGGER schema_meta_no_delete BEFORE DELETE ON schema_meta BEGIN SELECT RAISE(ABORT, 'append_only'); END;
CREATE TRIGGER schema_meta_no_replace BEFORE INSERT ON schema_meta
    WHEN EXISTS (SELECT 1 FROM schema_meta WHERE version = NEW.version)
    BEGIN SELECT RAISE(ABORT, 'append_only'); END;

CREATE TRIGGER entries_no_update BEFORE UPDATE ON entries BEGIN SELECT RAISE(ABORT, 'append_only'); END;
CREATE TRIGGER entries_no_delete BEFORE DELETE ON entries BEGIN SELECT RAISE(ABORT, 'append_only'); END;
CREATE TRIGGER entries_no_replace BEFORE INSERT ON entries
    WHEN EXISTS (SELECT 1 FROM entries WHERE seq = NEW.seq OR hash = NEW.hash)
    BEGIN SELECT RAISE(ABORT, 'append_only'); END;

CREATE TRIGGER entry_blobs_no_update BEFORE UPDATE ON entry_blobs BEGIN SELECT RAISE(ABORT, 'append_only'); END;
CREATE TRIGGER entry_blobs_no_delete BEFORE DELETE ON entry_blobs BEGIN SELECT RAISE(ABORT, 'append_only'); END;
CREATE TRIGGER entry_blobs_no_replace BEFORE INSERT ON entry_blobs
    WHEN EXISTS (SELECT 1 FROM entry_blobs
        WHERE entry_hash = NEW.entry_hash AND role = NEW.role AND digest = NEW.digest)
    BEGIN SELECT RAISE(ABORT, 'append_only'); END;

CREATE TRIGGER snapshots_no_update BEFORE UPDATE ON snapshots BEGIN SELECT RAISE(ABORT, 'append_only'); END;
CREATE TRIGGER snapshots_no_delete BEFORE DELETE ON snapshots BEGIN SELECT RAISE(ABORT, 'append_only'); END;
CREATE TRIGGER snapshots_no_replace BEFORE INSERT ON snapshots
    WHEN EXISTS (SELECT 1 FROM snapshots WHERE id = NEW.id)
    BEGIN SELECT RAISE(ABORT, 'append_only'); END;

CREATE TRIGGER checkpoints_no_update BEFORE UPDATE ON checkpoints BEGIN SELECT RAISE(ABORT, 'append_only'); END;
CREATE TRIGGER checkpoints_no_delete BEFORE DELETE ON checkpoints BEGIN SELECT RAISE(ABORT, 'append_only'); END;
CREATE TRIGGER checkpoints_no_replace BEFORE INSERT ON checkpoints
    WHEN EXISTS (SELECT 1 FROM checkpoints WHERE id = NEW.id)
    BEGIN SELECT RAISE(ABORT, 'append_only'); END;

CREATE TRIGGER anchors_no_update BEFORE UPDATE ON anchors BEGIN SELECT RAISE(ABORT, 'append_only'); END;
CREATE TRIGGER anchors_no_delete BEFORE DELETE ON anchors BEGIN SELECT RAISE(ABORT, 'append_only'); END;
CREATE TRIGGER anchors_no_replace BEFORE INSERT ON anchors
    WHEN EXISTS (SELECT 1 FROM anchors WHERE id = NEW.id)
    BEGIN SELECT RAISE(ABORT, 'append_only'); END;
