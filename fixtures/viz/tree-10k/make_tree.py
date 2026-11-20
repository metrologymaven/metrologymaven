#!/usr/bin/env python3
"""Generator and reference model for fixtures/viz/tree-10k (spec node VIZ.SHELL.SPKTREE).

Writes two files next to this script:

* ``tree.tsv``: 10 000 synthetic tree items, 6 levels deep, from SplitMix64 seeded with 5.
* ``keyboard.tsv``: a scripted keyboard sequence (AC-3) with the focus, selection and focused
  item name expected after each step.

The expected values come from the navigation rules written out below (the WAI-ARIA tree
pattern as the spike's crate documents it), implemented here independently of the Rust
crate. Run ``python make_tree.py --check`` to verify that the committed files match.

Stdlib only. The generator does not use ``random`` so the output does not depend on the
Python version.
"""

import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
SEED = 5
# Items per level, root level first: 6 levels, 10 000 items in total.
LEVEL_SIZES = [4, 16, 80, 400, 1900, 7600]
LEVEL_KINDS = ["part", "group", "group", None, "characteristic", "point"]
FEATURE_KINDS = ["plane", "cylinder", "circle", "cone", "sphere", "line"]
STATUSES = ["pass", "warn", "fail", "none"]
MASK = (1 << 64) - 1
MAX_NAME_BYTES = 256


class SplitMix64:
    def __init__(self, seed):
        self.state = seed & MASK

    def next(self):
        self.state = (self.state + 0x9E3779B97F4A7C15) & MASK
        z = self.state
        z = ((z ^ (z >> 30)) * 0xBF58476D1CE4E5B9) & MASK
        z = ((z ^ (z >> 27)) * 0x94D049BB133111EB) & MASK
        return z ^ (z >> 31)

    def below(self, n):
        return self.next() % n


def hostile_name(level, ordinal):
    """A few names that must be shown escaped (CONSTITUTION C6: data, never instructions)."""
    if level == 5 and ordinal == 8:
        return "point 8 \u202egnp.exe"
    if level == 5 and ordinal == 78:
        return "point\u200b78"
    if level == 5 and ordinal == 778:
        return "C:\\probe\\point 778"
    return None


def generate():
    rng = SplitMix64(SEED)
    # nodes[level] = list of dicts in creation order.
    nodes = []
    for level, size in enumerate(LEVEL_SIZES):
        row = []
        for j in range(size):
            if level == 0:
                parent = None
            else:
                prev = len(nodes[level - 1])
                # Every node of the previous level gets at least one child, so every root
                # reaches the full depth; the rest pick a parent at random.
                parent = j if j < prev else rng.below(prev)
            kind = LEVEL_KINDS[level]
            if kind is None:
                kind = FEATURE_KINDS[rng.below(len(FEATURE_KINDS))]
            status = STATUSES[rng.below(len(STATUSES))]
            name = hostile_name(level, j) or "%s %d" % (kind, j + 1)
            node = {"level": level, "parent": parent, "kind": kind, "status": status, "name": name, "children": []}
            if parent is not None:
                nodes[level - 1][parent]["children"].append(node)
            row.append(node)
        nodes.append(row)
    # Pre-order ids, siblings in creation order.
    out = []
    stack = list(reversed(nodes[0]))
    while stack:
        n = stack.pop()
        n["id"] = len(out) + 1
        out.append(n)
        stack.extend(reversed(n["children"]))
    return out


def tree_tsv(items):
    lines = ["# fid-spike-ui tree v1 seed=%d items=%d levels=%d" % (SEED, len(items), len(LEVEL_SIZES))]
    for n in items:
        pid = 0
        if n["level"] > 0:
            pid = n["parent_node"]["id"]
        lines.append("%d\t%d\t%s\t%s\t%s" % (n["id"], pid, n["kind"], n["status"], n["name"]))
    return "\n".join(lines) + "\n"


def link_parents(items):
    for n in items:
        for c in n["children"]:
            c["parent_node"] = n


# ---------------------------------------------------------------------------------------
# Reference navigation model. Rules (also documented in crates/fid-spike-ui/src/view.rs):
#
# Rows are the visible items in pre-order. Without a filter an item is visible when every
# ancestor is expanded. With a filter, an item is visible when its name (ASCII
# case-insensitive) contains the filter or a descendant's does; expanded flags are ignored.
#
# Down/Up/Home/End: move focus (no wrap); selection = {focus}; anchor = focus.
# Right: collapsed parent -> expand; expanded parent (or any parent while a filter is set)
#        -> focus and select the next row if it is a child of the focused item; leaf -> nothing.
# Left: expanded parent with no filter set -> collapse; otherwise -> focus and select the
#        parent (roots: nothing).
# ShiftDown/ShiftUp: move focus one row; selection = rows from anchor to focus (anchor
#        hidden: anchor = old focus first). No move -> nothing.
# CtrlDown/CtrlUp: move focus one row; selection unchanged.
# Space: toggle focus in the selection; anchor = focus.
# ExpandAll / CollapseAll: every parent expanded / collapsed; CollapseAll moves focus to the
#        root ancestor of the focused item.
# Filter <text>: set the filter; if the focus is no longer visible it moves to the first row
#        (none if there are no rows).
# F2: start renaming the focused item (buffer = its name, or empty when showing the name
#        needs escaping). While renaming only Type, Enter
#        and Escape act. Type <text>: replace the buffer. Enter: commit if the buffer is a
#        valid name (1..=256 bytes, no tab, CR or LF), else keep the old name (with a filter
#        set the rows are rebuilt and the focus rule of Filter applies); Escape: cancel.
# With no focus, every move key focuses the first row (and selects it).
# ---------------------------------------------------------------------------------------


def ascii_lower(s):
    return "".join(chr(ord(c) + 32) if "A" <= c <= "Z" else c for c in s)


def valid_name(s):
    b = s.encode("utf-8")
    return 0 < len(b) <= MAX_NAME_BYTES and "\t" not in s and "\r" not in s and "\n" not in s


def needs_escape(s):
    """True when showing ``s`` needs escaping (backslash, control, bidi, zero-width, separators)."""
    for c in s:
        u = ord(c)
        if (
            c == "\\"
            or u < 0x20
            or 0x7F <= u <= 0x9F
            or 0x200B <= u <= 0x200F
            or 0x2028 <= u <= 0x202E
            or 0x2060 <= u <= 0x206F
            or u == 0xFEFF
        ):
            return True
    return False


class Model:
    def __init__(self, items):
        self.items = items
        self.by_id = {n["id"]: n for n in items}
        self.roots = [n for n in items if n["level"] == 0]
        self.expanded = set()
        self.filter = ""
        self.focus = None
        self.selection = set()
        self.anchor = None
        self.editing = None
        self.rebuild()
        self.focus = self.rows[0] if self.rows else None

    def rebuild(self):
        rows = []
        keep = None
        if self.filter:
            f = ascii_lower(self.filter)
            keep = set()
            for n in reversed(self.items):
                if f in ascii_lower(n["name"]) or n["id"] in keep:
                    keep.add(n["id"])
                    if n["level"] > 0:
                        keep.add(n["parent_node"]["id"])
        stack = list(reversed(self.roots))
        while stack:
            n = stack.pop()
            if keep is not None and n["id"] not in keep:
                continue
            rows.append(n["id"])
            if keep is not None or n["id"] in self.expanded:
                stack.extend(reversed(n["children"]))
        self.rows = rows
        self.pos = {i: k for k, i in enumerate(rows)}

    def move(self, delta):
        """Returns the new focus id or None when the move is not possible."""
        if not self.rows:
            return None
        if self.focus is None or self.focus not in self.pos:
            return self.rows[0]
        k = self.pos[self.focus] + delta
        if 0 <= k < len(self.rows):
            return self.rows[k]
        return None

    def focus_select(self, i):
        self.focus = i
        self.selection = {i}
        self.anchor = i

    def apply(self, key, arg):
        if self.editing is not None:
            eid, _ = self.editing
            if key == "Type":
                self.editing = (eid, arg)
            elif key == "Enter":
                if valid_name(self.editing[1]):
                    self.by_id[eid]["name"] = self.editing[1]
                    if self.filter:
                        self.rebuild()
                        if self.focus not in self.pos:
                            self.focus = self.rows[0] if self.rows else None
                self.editing = None
            elif key == "Escape":
                self.editing = None
            return
        if key in ("Down", "Up"):
            if self.focus is None and self.rows:
                self.focus_select(self.rows[0])
                return
            t = self.move(1 if key == "Down" else -1)
            if t is not None:
                self.focus_select(t)
        elif key in ("Home", "End"):
            if self.rows:
                self.focus_select(self.rows[0] if key == "Home" else self.rows[-1])
        elif key == "Right":
            if self.focus is None:
                if self.rows:
                    self.focus_select(self.rows[0])
                return
            n = self.by_id[self.focus]
            if not n["children"]:
                return
            if self.filter or n["id"] in self.expanded:
                t = self.move(1)
                if t is not None and self.by_id[t]["level"] > 0 and self.by_id[t]["parent_node"] is n:
                    self.focus_select(t)
            else:
                self.expanded.add(n["id"])
                self.rebuild()
        elif key == "Left":
            if self.focus is None:
                if self.rows:
                    self.focus_select(self.rows[0])
                return
            n = self.by_id[self.focus]
            if not self.filter and n["children"] and n["id"] in self.expanded:
                self.expanded.discard(n["id"])
                self.rebuild()
            elif n["level"] > 0:
                self.focus_select(n["parent_node"]["id"])
        elif key in ("ShiftDown", "ShiftUp"):
            if self.focus is None:
                if self.rows:
                    self.focus_select(self.rows[0])
                return
            t = self.move(1 if key == "ShiftDown" else -1)
            if t is None:
                return
            if self.anchor is None or self.anchor not in self.pos:
                self.anchor = self.focus
            self.focus = t
            a, b = sorted((self.pos[self.anchor], self.pos[t]))
            self.selection = set(self.rows[a : b + 1])
        elif key in ("CtrlDown", "CtrlUp"):
            if self.focus is None:
                if self.rows:
                    self.focus_select(self.rows[0])
                return
            t = self.move(1 if key == "CtrlDown" else -1)
            if t is not None:
                self.focus = t
        elif key == "Space":
            if self.focus is not None:
                self.selection ^= {self.focus}
                self.anchor = self.focus
        elif key == "ExpandAll":
            self.expanded = {n["id"] for n in self.items if n["children"]}
            self.rebuild()
        elif key == "CollapseAll":
            self.expanded = set()
            if self.focus is not None:
                n = self.by_id[self.focus]
                while n["level"] > 0:
                    n = n["parent_node"]
                self.focus = n["id"]
            self.rebuild()
        elif key == "Filter":
            self.filter = arg
            self.rebuild()
            if self.focus is None or self.focus not in self.pos:
                self.focus = self.rows[0] if self.rows else None
        elif key == "F2":
            if self.focus is not None:
                name = self.by_id[self.focus]["name"]
                self.editing = (self.focus, "" if needs_escape(name) else name)
        else:
            raise ValueError("unknown key %r" % key)


SCRIPT = [
    ("Down", ""),
    ("Up", ""),
    ("Right", ""),
    ("Right", ""),
    ("Right", ""),
    ("Right", ""),
    ("ShiftDown", ""),
    ("ShiftDown", ""),
    ("CtrlDown", ""),
    ("Space", ""),
    ("CtrlUp", ""),
    ("ShiftUp", ""),
    ("Left", ""),
    ("Left", ""),
    ("Left", ""),
    ("F2", ""),
    ("Down", ""),
    ("Type", "Bolt hole A"),
    ("Enter", ""),
    ("Down", ""),
    ("F2", ""),
    ("Type", ""),
    ("Enter", ""),
    ("F2", ""),
    ("Type", "discarded"),
    ("Escape", ""),
    ("End", ""),
    ("Left", ""),
    ("Home", ""),
    ("ExpandAll", ""),
    ("End", ""),
    ("Up", ""),
    ("Left", ""),
    ("CollapseAll", ""),
    ("Up", ""),
    ("Filter", "CYLINDER 12"),
    ("Home", ""),
    ("Down", ""),
    ("Right", ""),
    ("End", ""),
    ("Left", ""),
    ("Filter", "no such item"),
    ("Down", ""),
    ("Filter", ""),
    ("Down", ""),
    ("Space", ""),
    ("Space", ""),
    ("Up", ""),
]


def keyboard_tsv(items):
    m = Model(items)
    lines = [
        "# fid-spike-ui keyboard v1: step, key, argument, expected focus, expected selection, focused name",
        "# start: everything collapsed, no filter, focus %s, empty selection" % m.focus,
    ]
    for step, (key, arg) in enumerate(SCRIPT, 1):
        m.apply(key, arg)
        focus = "-" if m.focus is None else str(m.focus)
        sel = ",".join(str(i) for i in sorted(m.selection)) or "-"
        name = "-" if m.focus is None else m.by_id[m.focus]["name"]
        lines.append("%d\t%s\t%s\t%s\t%s\t%s" % (step, key, arg, focus, sel, name))
    return "\n".join(lines) + "\n"


def build():
    items = generate()
    link_parents(items)
    tree = tree_tsv(items)
    # The keyboard model renames items; give it a fresh copy.
    items2 = generate()
    link_parents(items2)
    return {"tree.tsv": tree, "keyboard.tsv": keyboard_tsv(items2)}


def main(argv):
    check = "--check" in argv[1:]
    bad = 0
    for name, text in build().items():
        path = os.path.join(HERE, name)
        data = text.encode("utf-8")
        if check:
            with open(path, "rb") as f:
                if f.read() != data:
                    print("stale: %s" % name)
                    bad += 1
        else:
            with open(path, "wb") as f:
                f.write(data)
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
