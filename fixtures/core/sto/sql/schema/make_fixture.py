#!/usr/bin/env python3
"""Generate the CORE.STO.SQL.SCHEMA fixture database (AC-3, AC-4) with Python's sqlite3.

The schema is the v1 DDL file the Rust crate compiles in (crates/fid-store/src/schema/v1.sql),
executed here by Python's own SQLite, not by the code under test. The script then records
schema version 1 in schema_meta exactly as the migration runner does (one row, version 1),
inserts 100 seeded rows and leaves the file in WAL journal mode, fully checkpointed.

Rows (all values derived from the row number; digests are Python hashlib SHA-256):
  entries          40  seq 1..40; hash = sha256("entry-<seq>"); stream "activity" when
                       seq % 5 == 0 else "record"; branch "main" for seq <= 30 else "b1";
                       kind "session_header" for seq 1 else "command"; prev_hash = hash of the
                       previous entry of the same stream (NULL for the first); header =
                       b'{"seq":<seq>}'
  entry_blobs      20  entries 1..20, role "payload", digest = sha256("blob-<seq>")
  snapshots        10  entries 4, 8, ..., 40; format 1; state = sha256("state-<seq>");
                       content = sha256("content-<seq>"); size = 100 * seq
  checkpoints      10  id 1..10 at entries 4k; stream "record"; body = b"checkpoint-<k>"
  anchors           5  id 1..5 on checkpoints 1..5; kind "rfc3161"; body = b"anchor-<k>"
  heads             5  branches main, b1, b2, b3, b4 at entries 30, 40, 10, 20, 25
  idempotency       5  key "key-<k>" -> entry 2k
  settings_current  5  key "setting.<k>" -> value b'"v<k>"', entry 3k
Total: 100 rows (schema_meta's one row not counted).

Outputs (CC-BY-4.0 project data):
  v1.db    the database (binary; its bytes depend on the SQLite version that wrote it)
  v1.json  row count per table, written from the plan above, not read back from SQLite

Run:   python fixtures/core/sto/sql/schema/make_fixture.py
Check: python fixtures/core/sto/sql/schema/make_fixture.py --check
       (compares the logical content, i.e. sqlite3 iterdump, of a fresh build with v1.db, since
       the file bytes differ between SQLite versions)
"""

import hashlib
import json
import sqlite3
import sys
import tempfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[4]
DDL = ROOT / "crates" / "fid-store" / "src" / "schema" / "v1.sql"

COUNTS = {
    "entries": 40,
    "entry_blobs": 20,
    "snapshots": 10,
    "checkpoints": 10,
    "anchors": 5,
    "heads": 5,
    "idempotency": 5,
    "settings_current": 5,
}


def sha(text):
    return "sha256:" + hashlib.sha256(text.encode("ascii")).hexdigest()


def entry_hash(seq):
    return sha(f"entry-{seq}")


def build(path):
    con = sqlite3.connect(path, isolation_level=None)
    con.execute("PRAGMA foreign_keys=ON")
    mode = con.execute("PRAGMA journal_mode=WAL").fetchone()[0]
    assert mode == "wal", mode
    con.execute("BEGIN IMMEDIATE")
    for stmt in split_statements(DDL.read_text(encoding="utf-8")):
        con.execute(stmt)
    con.execute("INSERT INTO schema_meta (version) VALUES (1)")
    last = {}
    for seq in range(1, 41):
        stream = "activity" if seq % 5 == 0 else "record"
        con.execute(
            "INSERT INTO entries (seq, hash, stream, branch, kind, prev_hash, header) VALUES (?,?,?,?,?,?,?)",
            (
                seq,
                entry_hash(seq),
                stream,
                "main" if seq <= 30 else "b1",
                "session_header" if seq == 1 else "command",
                last.get(stream),
                ('{"seq":%d}' % seq).encode("ascii"),
            ),
        )
        last[stream] = entry_hash(seq)
    for seq in range(1, 21):
        con.execute(
            "INSERT INTO entry_blobs (entry_hash, role, digest) VALUES (?,?,?)",
            (entry_hash(seq), "payload", sha(f"blob-{seq}")),
        )
    for seq in range(4, 41, 4):
        con.execute(
            "INSERT INTO snapshots (entry_hash, format_version, state_digest, content_digest, size) "
            "VALUES (?,?,?,?,?)",
            (entry_hash(seq), 1, sha(f"state-{seq}"), sha(f"content-{seq}"), 100 * seq),
        )
    for k in range(1, 11):
        con.execute(
            "INSERT INTO checkpoints (id, stream, entry_hash, body) VALUES (?,?,?,?)",
            (k, "record", entry_hash(4 * k), f"checkpoint-{k}".encode("ascii")),
        )
    for k in range(1, 6):
        con.execute(
            "INSERT INTO anchors (id, checkpoint_id, kind, body) VALUES (?,?,?,?)",
            (k, k, "rfc3161", f"anchor-{k}".encode("ascii")),
        )
    for branch, seq in [("main", 30), ("b1", 40), ("b2", 10), ("b3", 20), ("b4", 25)]:
        con.execute("INSERT INTO heads (branch, entry_hash) VALUES (?,?)", (branch, entry_hash(seq)))
    for k in range(1, 6):
        con.execute("INSERT INTO idempotency (key, entry_hash) VALUES (?,?)", (f"key-{k}", entry_hash(2 * k)))
    for k in range(1, 6):
        con.execute(
            "INSERT INTO settings_current (key, value, entry_hash) VALUES (?,?,?)",
            (f"setting.{k}", f'"v{k}"'.encode("ascii"), entry_hash(3 * k)),
        )
    assert con.execute("PRAGMA foreign_key_check").fetchall() == []
    con.execute("COMMIT")
    con.execute("PRAGMA wal_checkpoint(TRUNCATE)")
    con.close()


def split_statements(sql):
    """Splits the DDL into complete statements with sqlite3.complete_statement."""
    out, buf = [], ""
    for line in sql.splitlines(keepends=True):
        if not buf and (line.strip() == "" or line.lstrip().startswith("--")):
            continue
        buf += line
        if sqlite3.complete_statement(buf):
            out.append(buf.strip())
            buf = ""
    assert buf.strip() == "", "trailing incomplete statement"
    return out


def dump(path):
    con = sqlite3.connect(f"file:{Path(path).as_posix()}?mode=ro&immutable=1", uri=True)
    try:
        return list(con.iterdump())
    finally:
        con.close()


def manifest_text():
    data = {"counts": COUNTS, "total": sum(COUNTS.values()), "schema_version": 1}
    return json.dumps(data, indent=2, sort_keys=True) + "\n"


def main():
    check = sys.argv[1:] == ["--check"]
    with tempfile.TemporaryDirectory() as tmp:
        fresh = Path(tmp) / "v1.db"
        build(str(fresh))
        if check:
            ok = dump(str(fresh)) == dump(str(HERE / "v1.db"))
            ok = ok and (HERE / "v1.json").read_text(encoding="utf-8") == manifest_text()
            print("v1.db and v1.json up to date" if ok else "v1.db or v1.json is stale")
            return 0 if ok else 1
        (HERE / "v1.db").write_bytes(fresh.read_bytes())
    (HERE / "v1.json").write_text(manifest_text(), encoding="utf-8", newline="\n")
    print("wrote v1.db and v1.json")
    return 0


if __name__ == "__main__":
    sys.exit(main())
