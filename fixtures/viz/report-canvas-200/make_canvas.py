#!/usr/bin/env python3
"""Generator and reference model for fixtures/viz/report-canvas-200 (spec node VIZ.SHELL.SPKRPT).

Writes four files next to this script:

* ``canvas.tsv``: 5 A4 pages holding 200 synthetic blocks (bands, text blocks, tables and one
  live 3D frame), from SplitMix64 seeded with 13.
* ``keyboard.tsv``: the scripted keyboard sequence of AC-3 with the state expected after
  each step (focus, focused block rectangle, editing flag, undo and redo depth, zoom and the
  focused block's text).
* ``drag.tsv``: pointer-drag cases with the position expected after snapping to the grid
  and to guides.
* ``a11y.tsv``: the role and the label (type, name and position) each block announces to a
  screen reader.

The expected values come from the canvas rules written out below, implemented here
independently of the Rust crate (``crates/fid-spike-ui/src/rpt``). Run
``python make_canvas.py --check`` to verify that the committed files match.

Stdlib only. The generator does not use ``random`` so the output does not depend on the
Python version. Lengths are integers in tenths of a millimetre (dmm) so both sides compute
exactly.

Canvas rules (the crate documents the same rules in ``rpt/edit.rs``):

* Paper: A4 is 2100 x 2970 dmm, Letter 2159 x 2794 dmm. Grid step 50 dmm (5 mm); guide
  tolerance 20 dmm; smallest block side 50 dmm; page margin guides 150 dmm from each edge.
* Focus starts on block 1 and is never empty. Tab / Shift+Tab: next / previous block id (no
  wrap). PageDown / PageUp: the lowest block id on the next / previous page (none: nothing).
* Arrows move the focused block one grid step, clamped to the page (0 <= x <= page_w - w).
  Shift+Right / Shift+Left widen / narrow by one grid step, Shift+Down / Shift+Up make it
  taller / shorter, clamped to [50, page_w - x] (heights alike). A change pushes one undo
  entry and clears the redo stack; no change pushes nothing.
* Enter on a band or text block starts editing with the block's text as buffer; on a table
  or 3D frame it does nothing. While editing only Insert (append to the buffer, at most 4096
  characters in total), SetText (replace the buffer) and Escape act. Escape ends editing and
  commits the buffer as one undo entry if it differs from the block's text.
* Undo pops the newest entry, restores the block's earlier state, pushes the entry on the
  redo stack and focuses the block. Redo is the reverse. The undo stack keeps 1000 entries.
* Zoom in / out: the next zoom step above / below the current zoom among
  25, 33, 50, 67, 75, 100, 125, 150, 200, 300, 400 percent (clamped). Start: 100.
* Drag with snapping: the raw position snaps to a guide when the block's left edge, centre
  or right edge (in that order; centre = x + w // 2) lies within 20 dmm of a guide; guides,
  in ascending order, are 0, the margins, the page centre and the left edge, centre and
  right edge of every other block on the page. The nearest guide wins; on a tie the first
  edge, then the smaller guide. Without a guide the position rounds to the nearest grid
  line (halves round up). The result is clamped to the page; a guide is reported only when
  the clamp did not move the block. y works the same way with heights. Without snapping
  the raw position is only clamped and no guide is reported.
"""

import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
SEED = 13
MASK = (1 << 64) - 1

PAPER = {"A4": (2100, 2970), "Letter": (2159, 2794)}
GRID = 50
GUIDE_TOL = 20
MIN_SIDE = 50
MARGIN = 150
MAX_TEXT_CHARS = 4096
UNDO_DEPTH = 1000
ZOOM_STEPS = [25, 33, 50, 67, 75, 100, 125, 150, 200, 300, 400]

PAGES = 5
COLS, ROWS = 3, 14
COL_PITCH, BODY_W = 600, 575
ROW_PITCH, BODY_H = 170, 150
BODY_X0, BODY_Y0 = 150, 300
BODY_PER_PAGE = 38

WORDS = [
    "probe", "datum", "plane", "cylinder", "flatness", "position", "report", "summary",
    "operator", "fixture", "alignment", "program", "section", "notes", "測定", "報告",
    "そくてい", "部品", "Prüfung", "tolérance",
]

KIND_WORD = {"band": "Band", "text": "Text block", "table": "Table", "frame3d": "3D view"}
ROLE = {"band": "group", "text": "paragraph", "table": "table", "frame3d": "image"}


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


# ---------------------------------------------------------------------------------------
# Canvas generation


def generate(rng):
    pages = ["A4"] * PAGES
    blocks = []
    counts = {"band": 0, "text": 0, "table": 0, "frame3d": 0}

    def add(page, kind, x, y, w, h, content):
        counts[kind] += 1
        if kind == "band":
            name = ("Header " if y < 1000 else "Footer ") + str(page)
        elif kind == "frame3d":
            name = "Viewport 1"
        else:
            name = ("Text " if kind == "text" else "Table ") + str(counts[kind])
        blocks.append({
            "id": len(blocks) + 1, "page": page, "kind": kind,
            "x": x, "y": y, "w": w, "h": h, "name": name, "content": content,
        })

    def words(n):
        return " ".join(WORDS[rng.below(len(WORDS))] for _ in range(n))

    for page in range(1, PAGES + 1):
        add(page, "band", MARGIN, 100, 2100 - 2 * MARGIN, 150, "Inspection report, page " + str(page))
        cells = [(r, c) for r in range(ROWS) for c in range(COLS)]
        body = BODY_PER_PAGE
        if page == 1:
            # The 3D frame spans 2 x 2 cells and counts as one of the page's body blocks.
            add(page, "frame3d", BODY_X0, BODY_Y0, COL_PITCH + BODY_W, ROW_PITCH + BODY_H, "-")
            cells = [(r, c) for (r, c) in cells if not (r < 2 and c < 2)]
            body -= 1
        for (r, c) in cells[:body]:
            x = BODY_X0 + c * COL_PITCH
            y = BODY_Y0 + r * ROW_PITCH
            if rng.below(5) == 0:
                add(page, "table", x, y, BODY_W, BODY_H, "%dx%d" % (2 + rng.below(5), 2 + rng.below(4)))
            else:
                add(page, "text", x, y, BODY_W, BODY_H, words(3 + rng.below(8)))
        add(page, "band", MARGIN, 2720, 2100 - 2 * MARGIN, 150, "Page " + str(page) + " of " + str(PAGES))
    return pages, blocks


def canvas_text(pages, blocks):
    out = ["# fid-spike-ui report canvas v1 (make_canvas.py, seed 13); lengths in 0.1 mm",
           "# page<TAB>number<TAB>paper",
           "# block<TAB>id<TAB>page<TAB>kind<TAB>x<TAB>y<TAB>w<TAB>h<TAB>name<TAB>content"]
    for i, p in enumerate(pages):
        out.append("page\t%d\t%s" % (i + 1, p))
    for b in blocks:
        out.append("block\t%d\t%d\t%s\t%d\t%d\t%d\t%d\t%s\t%s" % (
            b["id"], b["page"], b["kind"], b["x"], b["y"], b["w"], b["h"], b["name"], b["content"]))
    return "\n".join(out) + "\n"


# ---------------------------------------------------------------------------------------
# Reference designer (independent of the Rust crate)


class Designer:
    def __init__(self, pages, blocks):
        self.pages = pages
        self.blocks = [dict(b) for b in blocks]
        self.focus = 1
        self.editing = None  # (id, buffer)
        self.undo = []
        self.redo = []
        self.zoom = 100

    def block(self, bid):
        return self.blocks[bid - 1]

    def size(self, page):
        return PAPER[self.pages[page - 1]]

    def snapshot(self, b):
        return (b["x"], b["y"], b["w"], b["h"], b["content"])

    def restore(self, b, s):
        b["x"], b["y"], b["w"], b["h"], b["content"] = s

    def record(self, bid, before):
        after = self.snapshot(self.block(bid))
        if after == before:
            return
        self.undo.append((bid, before, after))
        if len(self.undo) > UNDO_DEPTH:
            self.undo.pop(0)
        self.redo = []

    def apply(self, key, arg):
        if self.editing is not None:
            bid, buf = self.editing
            if key == "Insert":
                self.editing = (bid, (buf + arg)[:MAX_TEXT_CHARS])
            elif key == "SetText":
                self.editing = (bid, arg[:MAX_TEXT_CHARS])
            elif key == "Escape":
                b = self.block(bid)
                before = self.snapshot(b)
                b["content"] = buf
                self.editing = None
                self.record(bid, before)
            return
        b = self.block(self.focus)
        pw, ph = self.size(b["page"])
        before = self.snapshot(b)
        if key == "Tab":
            self.focus = min(self.focus + 1, len(self.blocks))
        elif key == "BackTab":
            self.focus = max(self.focus - 1, 1)
        elif key in ("PageDown", "PageUp"):
            target = b["page"] + (1 if key == "PageDown" else -1)
            ids = [x["id"] for x in self.blocks if x["page"] == target]
            if ids:
                self.focus = min(ids)
        elif key in ("Left", "Right", "Up", "Down"):
            dx = {"Left": -GRID, "Right": GRID}.get(key, 0)
            dy = {"Up": -GRID, "Down": GRID}.get(key, 0)
            b["x"] = max(0, min(b["x"] + dx, pw - b["w"]))
            b["y"] = max(0, min(b["y"] + dy, ph - b["h"]))
            self.record(b["id"], before)
        elif key in ("ShiftLeft", "ShiftRight", "ShiftUp", "ShiftDown"):
            dw = {"ShiftLeft": -GRID, "ShiftRight": GRID}.get(key, 0)
            dh = {"ShiftUp": -GRID, "ShiftDown": GRID}.get(key, 0)
            b["w"] = max(MIN_SIDE, min(b["w"] + dw, pw - b["x"]))
            b["h"] = max(MIN_SIDE, min(b["h"] + dh, ph - b["y"]))
            self.record(b["id"], before)
        elif key == "Enter":
            if b["kind"] in ("band", "text"):
                self.editing = (b["id"], b["content"])
        elif key == "Undo":
            if self.undo:
                bid, old, new = self.undo.pop()
                self.restore(self.block(bid), old)
                self.redo.append((bid, old, new))
                self.focus = bid
        elif key == "Redo":
            if self.redo:
                bid, old, new = self.redo.pop()
                self.restore(self.block(bid), new)
                self.undo.append((bid, old, new))
                self.focus = bid
        elif key == "ZoomIn":
            self.zoom = next((z for z in ZOOM_STEPS if z > self.zoom), ZOOM_STEPS[-1])
        elif key == "ZoomOut":
            self.zoom = next((z for z in reversed(ZOOM_STEPS) if z < self.zoom), ZOOM_STEPS[0])
        elif key == "Escape":
            pass
        else:
            raise ValueError(key)

    def state_line(self):
        b = self.block(self.focus)
        content = b["content"] if b["kind"] in ("band", "text") else "-"
        return "%d\t%d\t%d\t%d\t%d\t%d\t%d\t%d\t%d\t%d\t%s" % (
            self.focus, b["page"], b["x"], b["y"], b["w"], b["h"], 1 if self.editing else 0,
            len(self.undo), len(self.redo), self.zoom, content)


def snap_axis(raw, size, extent, guides):
    best = None  # (distance, edge index, guide)
    for ei, off in enumerate((0, size // 2, size)):
        for g in guides:
            d = abs(raw + off - g)
            if d <= GUIDE_TOL and (best is None or d < best[0]):
                best = (d, ei, g, off)
    if best is not None:
        pos, guide = best[2] - best[3], best[2]
    else:
        pos, guide = ((raw + GRID // 2) // GRID) * GRID, None
    clamped = max(0, min(pos, extent - size))
    # A guide the clamp moved the block away from is not shown.
    return clamped, (guide if clamped == pos else None)


def drag(designer, bid, raw_x, raw_y, snap):
    b = designer.block(bid)
    pw, ph = designer.size(b["page"])
    if not snap:
        return max(0, min(raw_x, pw - b["w"])), max(0, min(raw_y, ph - b["h"])), None, None
    others = [o for o in designer.blocks if o["page"] == b["page"] and o["id"] != bid]
    gx = sorted({0, MARGIN, pw - MARGIN, pw // 2}
                | {o["x"] for o in others} | {o["x"] + o["w"] // 2 for o in others}
                | {o["x"] + o["w"] for o in others})
    gy = sorted({0, MARGIN, ph - MARGIN, ph // 2}
                | {o["y"] for o in others} | {o["y"] + o["h"] // 2 for o in others}
                | {o["y"] + o["h"] for o in others})
    x, guide_x = snap_axis(raw_x, b["w"], pw, gx)
    y, guide_y = snap_axis(raw_y, b["h"], ph, gy)
    return x, y, guide_x, guide_y


def dmm(v):
    return "%d.%d" % (v // 10, v % 10)


def label(b, pages):
    return "%s %s, page %d of %d, x %s mm, y %s mm, %s by %s mm" % (
        KIND_WORD[b["kind"]], b["name"], b["page"], pages, dmm(b["x"]), dmm(b["y"]),
        dmm(b["w"]), dmm(b["h"]))


# ---------------------------------------------------------------------------------------
# Scripts


def keyboard_script(pages, blocks):
    """The AC-3 sequence: move a block 10 grid steps and resize it by keyboard alone."""
    steps = []

    def k(key, arg="", n=1):
        for _ in range(n):
            steps.append((key, arg))

    page2 = [b for b in blocks if b["page"] == 2]
    target = next(b for b in page2 if b["kind"] != "band")  # body cell row 0, column 0
    k("PageDown")
    k("Tab", n=target["id"] - page2[0]["id"])
    k("Right", n=10)            # AC-3: 10 grid steps
    k("Down", n=2)
    k("ShiftRight", n=3)        # AC-3: resize
    k("ShiftDown")
    k("Undo", n=2)
    k("Redo")
    k("Left", n=20)             # clamps at the page edge
    k("ShiftLeft", n=15)        # clamps at the smallest side
    k("ShiftUp", n=4)
    k("ZoomIn", n=6)
    k("ZoomOut", n=12)
    k("ZoomIn", n=5)
    k("Enter")                  # no effect unless text or band
    k("Escape")
    text = next(b for b in page2 if b["kind"] == "text" and b["id"] > target["id"])
    k("Tab", n=text["id"] - target["id"])
    k("Enter")
    k("Right")                  # ignored while editing
    k("Insert", " そくてい")     # committed IME text (AC-4 runs the real IME by hand)
    k("Escape")
    k("Undo")
    k("Redo")
    k("Enter")
    k("Escape")                 # unchanged text: no undo entry
    table = next((b for b in page2 if b["kind"] == "table" and b["id"] > text["id"]), None)
    if table is not None:
        k("Tab", n=table["id"] - text["id"])
        k("Enter")              # tables are not edited in the spike
        k("Down")
    k("Undo", n=3)
    k("Up")                     # a new edit clears the redo stack
    k("PageUp")
    k("PageUp")
    k("BackTab")
    k("Enter")
    k("SetText", "Inspection report")
    k("Escape")
    return steps


def keyboard_text(pages, blocks):
    d = Designer(pages, blocks)
    out = ["# fid-spike-ui canvas keyboard v1: step, key, argument, then after the step: focus, page,",
           "# x, y, w, h (0.1 mm), editing, undo depth, redo depth, zoom %, focused block text (- for none)",
           "# start: focus 1, zoom 100, nothing edited"]
    for n, (key, arg) in enumerate(keyboard_script(pages, blocks), start=1):
        d.apply(key, arg)
        out.append("%d\t%s\t%s\t%s" % (n, key, arg, d.state_line()))
    return "\n".join(out) + "\n"


def drag_text(pages, blocks, rng):
    d = Designer(pages, blocks)
    cases = []
    # Hand-picked: margins, the page centre, neighbouring edges, clamping and no snapping.
    for bid, x, y, snap in [
        (3, 160, 310, 1), (3, 140, 290, 1), (3, 0, 0, 1), (3, -400, -400, 1),
        (3, 5000, 5000, 1), (3, 1050, 2000, 1), (3, 777, 1234, 0), (3, -1, 9999, 0),
        (45, 1074, 471, 1), (45, 612, 476, 1), (45, 638, 488, 1), (45, 662, 488, 1),
    ]:
        cases.append((bid, x, y, snap))
    for _ in range(188):
        bid = 1 + rng.below(len(blocks))
        x = rng.below(2600) - 250
        y = rng.below(3400) - 200
        cases.append((bid, x, y, 1 if rng.below(8) else 0))
    out = ["# fid-spike-ui canvas drag v1: block, raw x, raw y, snap, expected x, y, guide x, guide y",
           "# (0.1 mm; - for no guide); each case starts from the fixture canvas"]
    for bid, x, y, snap in cases:
        ex, ey, gx, gy = drag(d, bid, x, y, snap == 1)
        out.append("%d\t%d\t%d\t%d\t%d\t%d\t%s\t%s" % (
            bid, x, y, snap, ex, ey, "-" if gx is None else gx, "-" if gy is None else gy))
    return "\n".join(out) + "\n"


def a11y_text(pages, blocks):
    out = ["# fid-spike-ui canvas a11y v1: block, role, label"]
    for b in blocks:
        out.append("%d\t%s\t%s" % (b["id"], ROLE[b["kind"]], label(b, len(pages))))
    return "\n".join(out) + "\n"


def build():
    rng = SplitMix64(SEED)
    pages, blocks = generate(rng)
    assert len(pages) == 5 and len(blocks) == 200
    assert sum(1 for b in blocks if b["kind"] == "frame3d") == 1
    for p in range(1, PAGES + 1):
        assert any(b["kind"] == "table" and b["page"] == p for b in blocks), p
    for b in blocks:
        pw, ph = PAPER[pages[b["page"] - 1]]
        assert 0 <= b["x"] and b["x"] + b["w"] <= pw and 0 <= b["y"] and b["y"] + b["h"] <= ph
    return {
        "canvas.tsv": canvas_text(pages, blocks),
        "keyboard.tsv": keyboard_text(pages, blocks),
        "drag.tsv": drag_text(pages, blocks, rng),
        "a11y.tsv": a11y_text(pages, blocks),
    }


def main(argv):
    check = "--check" in argv[1:]
    bad = 0
    for name, text in build().items():
        path = os.path.join(HERE, name)
        data = text.encode("utf-8")
        if check:
            try:
                with open(path, "rb") as f:
                    same = f.read() == data
            except OSError:
                same = False
            if not same:
                print("stale: " + path)
                bad += 1
        else:
            with open(path, "wb") as f:
                f.write(data)
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
