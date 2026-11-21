#!/usr/bin/env python3
"""Independent reference for VIZ.RENDER.SCENE.STALE AC-1, AC-2 and AC-4 (project-authored, CC0).

Builds on the scene-doc-small document of make_fixture.py (VIZ.RENDER.SCENE.PROJ), whose
projection and line layout it reuses (imported, not re-run Rust). Scenario, hand-labelled from
the leaf: an alignment (not drawn; entity ...0a1) that all 12 characteristics are evaluated in
is changed, so CORE.DEP.PROP marks the alignment and its 12 dependent characteristics
`outdated`. Writes, next to itself:

- stale-outdated.txt: the outdated set CORE publishes, one entity per line (alignment first,
  then c01..c12). The alignment has no scene node.
- stale-expect-mark-batch.txt: the change batch of marking: every characteristic node, label
  and leader (render IDs 5..40) updated, nothing added or retired.
- stale-expect-marked.txt / .digest: the serialised graph after marking. A stale node's status
  field is `stale:<flags>:<since digest>`: the c/d/u flags of the kernel status it replaced
  (`---` for not_evaluated) and the data digest the node displayed when it became stale.
- stale-doc-recomputed.txt: the document after recompute: each characteristic has a new data
  digest (version 2) and a new kernel status (the status cycle shifted by one); geometry and
  origins unchanged.
- stale-dirty.txt: the dirty set after recompute (the 12 characteristics).
- stale-expect-recompute-batch.txt: re-projecting stale-dirty.txt against the recomputed
  document with the 12 marks cleared: 5..40 updated.
- stale-expect-recomputed.txt / .digest: the graph after that batch: exactly the full projection
  of the recomputed document (no node stale).
- stale-expect-held.txt: the same re-projection with the marks NOT cleared: the nodes take the
  new data but stay stale, `since` unchanged, flags those of the delivered status.

AC-4 (same digest, marks cleared) re-projects doc.txt itself; its expected graph is
expect-full.txt of make_fixture.py and its batch is 5..40 updated (status text changes back).

Run:  python fixtures/viz/scene-doc-small/make_stale.py          (write)
      python fixtures/viz/scene-doc-small/make_stale.py --check  (compare, exit 1 on drift)
"""

import importlib.util
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent

_spec = importlib.util.spec_from_file_location("scene_doc_small", HERE / "make_fixture.py")
base = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(base)

ALIGNMENT = base.uuid(0xA1)

STATUSES = [
    "kernel:CONFORMS:---",
    "kernel:NONCONFORMS:---",
    "kernel:INDETERMINATE:--u",
    "kernel:CONFORMS:c--",
    "kernel:NOT_DECIDABLE(u_missing):-d-",
]


def flags(status):
    if status.startswith("kernel:"):
        return status.rsplit(":", 1)[1]
    assert status == "not_evaluated", status
    return "---"


def recomputed_document():
    els = base.document(False)
    out = []
    for e in els:
        if e["kind"] == "characteristic":
            i = int(e["entity"][-12:], 16) - 10
            assert e["status"] == STATUSES[i % 5]
            e = dict(e)
            e["status"] = STATUSES[(i + 1) % 5]
            e["digest"] = base.data_digest("c%02d" % i, 2)
        out.append(e)
    return out


def lines(els, stale_since):
    """Serialised graph of the full projection of els. stale_since maps a node identity to the
    since digest of a stale node; such a node's status becomes stale:<flags>:<since>."""
    rid = 0
    text = []
    for e in els:
        for node in base.project(e):
            rid += 1
            shown = e
            if node[0] in stale_since:
                shown = dict(e)
                shown["status"] = "stale:%s:%s" % (flags(e["status"]), stale_since[node[0]])
            text.append(base.node_line(rid, node, shown) + "\n")
    return "".join(text)


def node_digests(els, entities):
    """identity -> displayed data digest, for the nodes of the given entities."""
    return {node[0]: node[6] for e in els if e["entity"] in entities for node in base.project(e)}


def outputs():
    doc = base.document(False)
    chars = [e["entity"] for e in doc if e["kind"] == "characteristic"]
    assert len(chars) == 12
    outdated = [ALIGNMENT] + chars
    since = node_digests(doc, set(outdated))
    assert len(since) == 36
    marked = lines(doc, since)
    # Render IDs of the characteristic nodes: elements 5..16 in tree order, 3 nodes each.
    rids = list(range(5, 41))
    full, last = base.full_projection(doc)
    assert sorted(r for ident, (r, _) in full.items() if ident.split(":")[1] in chars) == rids
    assert last == 40
    re_doc = recomputed_document()
    recomputed = lines(re_doc, {})
    held = lines(re_doc, since)
    batch = ([], rids, [])
    return {
        "stale-outdated.txt": "".join(e + "\n" for e in outdated),
        "stale-expect-mark-batch.txt": base.batch_text(batch),
        "stale-expect-marked.txt": marked,
        "stale-expect-marked.digest": base.graph_digest(marked) + "\n",
        "stale-doc-recomputed.txt": base.doc_text(re_doc),
        "stale-dirty.txt": "".join(e + "\n" for e in chars),
        "stale-expect-recompute-batch.txt": base.batch_text(batch),
        "stale-expect-recomputed.txt": recomputed,
        "stale-expect-recomputed.digest": base.graph_digest(recomputed) + "\n",
        "stale-expect-held.txt": held,
    }


def main():
    check = "--check" in sys.argv[1:]
    drift = False
    for name, text in outputs().items():
        path = HERE / name
        if check:
            if not path.exists() or path.read_bytes() != text.encode():
                print("drift: " + name)
                drift = True
        else:
            path.write_bytes(text.encode())
    return 1 if drift else 0


if __name__ == "__main__":
    sys.exit(main())
