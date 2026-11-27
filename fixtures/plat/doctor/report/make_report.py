"""Independent reference oracle for PLAT.DOCTOR.REPORT (report JSON, text view, redaction).

Licensed with the code (MIT OR Apache-2.0, D-001). Python standard library only. Every input
is synthetic and invented here (D-038): no machine, person or product is real.

Implements the leaf's rules from spec/plat/doctor/report/NODE.md and the rules documented in
the `fid_doctor::report` module docs, written again here without reading the Rust code paths:

  report JSON  {assessment, phase, redacted, schema_version: 1, snapshot, tool_version} as
               RFC 8785 canonical JSON. The assessment is the PLAT.DOCTOR.LOOKUP shape (it
               carries the outcome and exit code); the snapshot is the PLAT.DOCTOR.PROBE shape.
               As PROBE serialises it, an unknown item's `detail` has control characters
               turned into spaces, whitespace runs collapsed and at most 200 characters.
  text view    header lines schema_version, tool_version, phase, redacted; then sections
               Platform (os, arch, tier, snapshot_schema_version), Hardware (cpu, ram,
               storage, display), GPU (gpu), Verdicts (outcome, exit_code,
               assessment_schema_version, profiles) and Unknowns. Values are walked: a scalar
               is `label: value`, an empty array `[]`, an empty object `{}`, an array of
               scalars `[a, b]`, anything else `label:` with members two spaces deeper
               (object keys in code point order, array elements labelled `[i]`). Strings are
               quoted with `"` and `\\` backslash-escaped and control/format characters as
               `\\u{hex}`; numbers in ECMAScript form; true/false/null as in JSON. Unknowns
               lists `item: reason "detail"` per unknown item (os, arch, tier, cpu, ram,
               storage, display, gpu), `profile "name": reason` and `profile "name": not
               compared: "f1", "f2"` per profile, or `none`.
  redaction    paths, then home, then user/host names (longest first); see `redact()`.

Outputs (all canonical JSON or plain text):
  fixtures/plat/doctor/report-win11.json / .txt   AC-1 / AC-2 (LOOKUP AC-1's assessment)
  fixtures/plat/doctor/report-schema99.json       AC-5
  fixtures/plat/doctor/report-escape.json / .txt  AC-6 (not canonical: ASCII-only, escaped)
  fixtures/plat/doctor/report/identifying.json    AC-3 input; .redacted.json the expected
                                                  redaction; identity in redact_cases.json
  fixtures/plat/doctor/report/redact_cases.json   string-level redaction cases, hand-labelled
  fixtures/plat/doctor/report/cases.json          one report per PLAT.DOCTOR.LOOKUP oracle
                                                  case with its expected text
  fixtures/plat/doctor/report/invalid_cases.json  hand-labelled invalid reports
  platform/doctor-report.schema.json              the report JSON Schema (2020-12), with the
                                                  PROBE snapshot schema embedded

Run from the repository root:
  python fixtures/plat/doctor/report/make_report.py          (write)
  python fixtures/plat/doctor/report/make_report.py --check  (exit 1 if any file differs)
"""

import copy
import json
import os
import sys
import unicodedata
from decimal import Decimal

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.normpath(os.path.join(HERE, "..", "..", "..", ".."))
DOCTOR = os.path.join(ROOT, "fixtures", "plat", "doctor")

TOOL_VERSION = "0.1.0"
MAX_DETAIL = 200

# ------------------------------------------------------------------ canonical JSON


def js_num(x):
    """ECMAScript Number::toString of a finite number (what RFC 8785 prints)."""
    if isinstance(x, bool):
        raise TypeError("bool is not a number")
    if isinstance(x, int):
        return str(x)
    if x != x or x in (float("inf"), float("-inf")):
        raise ValueError("non-finite number")
    if x == 0:
        return "0"
    if x < 0:
        return "-" + js_num(-x)
    sign, digits, exp = Decimal(repr(x)).as_tuple()
    digits = list(digits)
    while len(digits) > 1 and digits[-1] == 0:
        digits.pop()
        exp += 1
    while len(digits) > 1 and digits[0] == 0:
        digits.pop(0)
    s = "".join(str(d) for d in digits)
    k = len(s)
    n = k + exp
    if k <= n <= 21:
        return s + "0" * (n - k)
    if 0 < n <= 21:
        return s[:n] + "." + s[n:]
    if -6 < n <= 0:
        return "0." + "0" * (-n) + s
    e = n - 1
    es = ("+" if e >= 0 else "-") + str(abs(e))
    if k == 1:
        return s + "e" + es
    return s[0] + "." + s[1:] + "e" + es


def canon(v):
    """RFC 8785 canonical JSON text of a JSON value (keys in UTF-16 order; ASCII keys here)."""
    if v is None:
        return "null"
    if v is True:
        return "true"
    if v is False:
        return "false"
    if isinstance(v, (int, float)):
        return js_num(v)
    if isinstance(v, str):
        return json.dumps(v, ensure_ascii=False)
    if isinstance(v, list):
        return "[" + ",".join(canon(x) for x in v) + "]"
    if isinstance(v, dict):
        keys = sorted(v, key=lambda k: k.encode("utf-16-be"))
        return "{" + ",".join(json.dumps(k, ensure_ascii=False) + ":" + canon(v[k]) for k in keys) + "}"
    raise TypeError(type(v))


def canon_bytes(v):
    return canon(v).encode("utf-8")


# ------------------------------------------------------------------ PROBE detail normalisation

PROBED = ["os", "arch", "cpu", "ram", "storage", "display", "gpu"]


def is_cc(c):
    return unicodedata.category(c) == "Cc"


def bound_detail(s):
    cleaned = "".join(" " if is_cc(c) else c for c in s)
    return "".join(" ".join(cleaned.split())[:MAX_DETAIL])


def normalise(report):
    """The report as PROBE/LOOKUP serialise it: unknown details bounded."""
    r = copy.deepcopy(report)
    for item in PROBED:
        p = r["snapshot"][item]
        if p["status"] == "unknown":
            p["detail"] = bound_detail(p["detail"])
    t = r["assessment"]["tier"]
    if t["status"] == "unknown":
        t["detail"] = bound_detail(t["detail"])
    return r


# ------------------------------------------------------------------ text view


def is_escaped(c):
    o = ord(c)
    return (
        is_cc(c)
        or o == 0x061C
        or 0x200B <= o <= 0x200F
        or 0x2028 <= o <= 0x202E
        or 0x2060 <= o <= 0x2064
        or 0x2066 <= o <= 0x2069
        or o == 0xFEFF
    )


def esc(s):
    return "".join("\\u{%x}" % ord(c) if is_escaped(c) else c for c in s)


def quote(s):
    out = ['"']
    for c in s:
        if c == '"':
            out.append('\\"')
        elif c == "\\":
            out.append("\\\\")
        elif is_escaped(c):
            out.append("\\u{%x}" % ord(c))
        else:
            out.append(c)
    out.append('"')
    return "".join(out)


def scalar(v):
    if v is None:
        return "null"
    if v is True:
        return "true"
    if v is False:
        return "false"
    if isinstance(v, (int, float)):
        return js_num(v)
    if isinstance(v, str):
        return quote(v)
    return None


def walk(out, label, v, indent):
    pad = " " * indent
    s = scalar(v)
    if s is not None:
        out.append(f"{pad}{label}: {s}")
    elif isinstance(v, list):
        if not v:
            out.append(f"{pad}{label}: []")
        elif all(scalar(x) is not None for x in v):
            out.append(f"{pad}{label}: [" + ", ".join(scalar(x) for x in v) + "]")
        else:
            out.append(f"{pad}{label}:")
            for i, x in enumerate(v):
                walk(out, f"[{i}]", x, indent + 2)
    elif isinstance(v, dict):
        if not v:
            out.append(f"{pad}{label}: {{}}")
        else:
            out.append(f"{pad}{label}:")
            for k in sorted(v):
                walk(out, esc(k), v[k], indent + 2)


def render(report):
    r = normalise(report)
    snap, ass = r["snapshot"], r["assessment"]
    out = ["Metrology Maven doctor report"]
    for k in ["schema_version", "tool_version", "phase", "redacted"]:
        walk(out, k, r[k], 0)
    out += ["", "Platform"]
    walk(out, "os", snap["os"], 2)
    walk(out, "arch", snap["arch"], 2)
    walk(out, "tier", ass["tier"], 2)
    walk(out, "snapshot_schema_version", snap["schema_version"], 2)
    out += ["", "Hardware"]
    for k in ["cpu", "ram", "storage", "display"]:
        walk(out, k, snap[k], 2)
    out += ["", "GPU"]
    walk(out, "gpu", snap["gpu"], 2)
    out += ["", "Verdicts"]
    walk(out, "outcome", ass["outcome"], 2)
    walk(out, "exit_code", ass["exit_code"], 2)
    walk(out, "assessment_schema_version", ass["schema_version"], 2)
    walk(out, "profiles", ass["profiles"], 2)
    out += ["", "Unknowns"]
    lines = []
    for name, p in [
        ("os", snap["os"]),
        ("arch", snap["arch"]),
        ("tier", ass["tier"]),
        ("cpu", snap["cpu"]),
        ("ram", snap["ram"]),
        ("storage", snap["storage"]),
        ("display", snap["display"]),
        ("gpu", snap["gpu"]),
    ]:
        if p["status"] == "unknown":
            lines.append(f"  {name}: {esc(p['reason'])} {quote(p['detail'])}")
    for p in ass["profiles"]:
        if p["reason"] is not None:
            lines.append(f"  profile {quote(p['name'])}: {esc(p['reason'])}")
        if p["unknowns"]:
            lines.append(f"  profile {quote(p['name'])}: not compared: " + ", ".join(quote(f) for f in p["unknowns"]))
    out += lines if lines else ["  none"]
    return "\n".join(out) + "\n"


# ------------------------------------------------------------------ redaction

USER, HOST, HOME, PATH = "<redacted:user>", "<redacted:host>", "<redacted:home>", "<redacted:path>"

# Simple case foldings that differ from the single-character lowercase mapping.
FOLD_EXTRA = {
    "\u03c2": "\u03c3",  # final sigma
    "\u017f": "s",  # long s
    "\u0345": "\u03b9",
    "\u1fbe": "\u03b9",
    "\u03d0": "\u03b2",
    "\u03d1": "\u03b8",
    "\u03d5": "\u03c6",
    "\u03d6": "\u03c0",
    "\u03f0": "\u03ba",
    "\u03f1": "\u03c1",
    "\u03f5": "\u03b5",
    "\u1e9b": "\u1e61",
    "\u0130": "i",  # Turkish dotted capital I, folded to i (fail closed)
}


def fold(c):
    if c in FOLD_EXTRA:
        return FOLD_EXTRA[c]
    low = c.lower()
    return low if len(low) == 1 else c


def is_sep(c):
    return c in "/\\"


def is_term(c):
    return c.isspace() or is_escaped(c) or c in "\"'`<>|"


def is_line_break(c):
    return c in "\n\r\x0b\x0c\x85\u2028\u2029"


def is_alnum(c):
    return c.isalnum()


def is_path_char(c):
    return is_alnum(c) or c in "._-~"


def is_trailing_punct(c):
    return c in ".,;:!?)]}"


def fold_eq(a, b):
    return a == b or fold(a) == fold(b)


def path_eq(a, b, windows):
    if windows:
        return fold_eq(a, b) or (is_sep(a) and is_sep(b))
    return a == b


def starts_at(text, i, pat, eq):
    return i + len(pat) <= len(text) and all(eq(text[i + k], p) for k, p in enumerate(pat))


def abs_start(c, i):
    a = c[i] if i < len(c) else None
    b = c[i + 1] if i + 1 < len(c) else None
    d = c[i + 2] if i + 2 < len(c) else None
    if a is not None and a.isascii() and a.isalpha() and b == ":" and d is not None and is_sep(d):
        return True
    return a is not None and is_sep(a) and b is not None and not is_term(b)


def tilde_home(c, i):
    return i + 1 < len(c) and c[i] == "~" and is_sep(c[i + 1])


def verbatim_len(c, i):
    return 4 if c[i : i + 4] == "\\\\?\\" else 0


def is_absolute(c):
    return (len(c) >= 3 and c[0].isascii() and c[0].isalpha() and c[1] == ":" and is_sep(c[2])) or (
        len(c) >= 1 and is_sep(c[0])
    )


def strip_seps(c):
    while len(c) > 1 and is_sep(c[-1]):
        c = c[:-1]
    return c


def comps_after_root(c):
    parts = [p for p in c.replace("\\", "/").split("/") if p]
    if len(c) >= 2 and c[0].isascii() and c[0].isalpha() and c[1] == ":":
        skip = 1
    elif len(c) >= 2 and is_sep(c[0]) and is_sep(c[1]):
        skip = 2
    else:
        skip = 0
    return parts[skip:]


class RedactError(Exception):
    def __init__(self, code, detail):
        super().__init__(code, detail)
        self.code = code
        self.detail = detail


def make_redactor(identity, install_dir):
    user = identity["user"].strip()
    if not user:
        raise RedactError("identity_unavailable", "user")
    hosts = [h.strip() for h in identity["hosts"] if h.strip()]
    if not hosts:
        raise RedactError("identity_unavailable", "host")
    home = identity["home"].strip()
    if not is_absolute(home):
        raise RedactError("identity_unavailable", "home")
    home = strip_seps(home)
    home_comps = comps_after_root(home)
    home_last = home_comps[-1] if home_comps else None
    if not home_comps:
        home = ""
    inst = install_dir.strip()
    if not is_absolute(inst):
        raise RedactError("invalid_install_dir", "not an absolute path")
    inst = strip_seps(inst)
    comps = comps_after_root(inst)
    if not comps:
        raise RedactError("invalid_install_dir", "is a root directory")
    if ".." in comps:
        raise RedactError("invalid_install_dir", "has a `..` component")
    win = (len(inst) >= 2 and inst[0].isascii() and inst[0].isalpha() and inst[1] == ":") or inst.startswith("\\")
    if home and starts_at(home, 0, inst, lambda a, b: path_eq(a, b, True)) and (
        len(home) == len(inst) or is_sep(home[len(inst)])
    ):
        raise RedactError("invalid_install_dir", "contains the home directory")
    dirs = []
    for d in identity.get("user_dirs", []):
        d = d.strip()
        if is_absolute(d):
            d = strip_seps(d)
            if comps_after_root(d):
                dirs.append(d)
    names = []

    def push(s, mark):
        s = s.strip()
        if s and not any(len(n) == len(s) and all(fold_eq(a, b) for a, b in zip(n, s)) for n, _ in names):
            names.append((s, mark))

    def with_label(s):
        return [s, s.split(".", 1)[0]] if "." in s else [s]

    for h in hosts:
        for n in with_label(h):
            push(n, HOST)
    for d in identity.get("domains", []):
        for n in with_label(d.strip()):
            push(n, HOST)
    push(user, USER)
    if home_last is not None:
        push(home_last, USER)
    names.sort(key=lambda nm: -len(nm[0]))  # stable: ties keep the push order
    return {"install": inst, "win": win, "home": home, "dirs": dirs, "names": names}


def path_end(r, c, i):
    def scan(j):
        while j < len(c) and not is_term(c[j]):
            j += 1
        return j

    end, min_end = scan(i), i + 1
    base = i + verbatim_len(c, i)
    for p, win in [(r["install"], r["win"]), (r["home"], True)] + [(d, True) for d in r["dirs"]]:
        after = base + len(p)
        if p and starts_at(c, base, p, lambda a, b: path_eq(a, b, win)) and (
            after == len(c) or is_sep(c[after]) or is_term(c[after]) or is_trailing_punct(c[after])
        ):
            end = max(end, scan(after))
            min_end = max(min_end, after)
    while end > min_end and is_trailing_punct(c[end - 1]):
        end -= 1
    return end


def redacted_end(c, end, before):
    """How far a redacted path ending at `end` swallows the text after it."""
    if before is not None and before in "\"'`":
        j = end
        while j < len(c) and c[j] != before and not is_line_break(c[j]):
            j += 1
        return j
    # Look ahead over the whitespace-separated tokens of the same line, up to a token that
    # starts a path of its own; swallow through the last token holding a separator.
    best, j = end, end
    while True:
        while j < len(c) and c[j].isspace() and not is_line_break(c[j]):
            j += 1
        if j >= len(c) or is_line_break(c[j]) or (j > end and (abs_start(c, j) or tilde_home(c, j))):
            return best
        k = j
        while k < len(c) and not c[k].isspace():
            k += 1
        if any(is_sep(x) for x in c[j:k]):
            t = k
            while t > j + 1 and is_trailing_punct(c[t - 1]):
                t -= 1
            best = t
        j = k


def keeps(r, tok):
    tok = tok[verbatim_len(tok, 0) :]
    inst = r["install"]
    n = len(inst)
    return (
        starts_at(tok, 0, inst, lambda a, b: path_eq(a, b, r["win"]))
        and (len(tok) == n or is_sep(tok[n]))
        and ".." not in comps_after_root(tok)
    )


def redact(r, s):
    # segments: ("t", text) or ("m", placeholder)
    segs, buf, i = [], "", 0
    while i < len(s):
        before = s[i - 1] if i > 0 else None
        boundary = before is None or not (is_path_char(before) or is_sep(before))
        if boundary and tilde_home(s, i):
            if buf:
                segs.append(("t", buf))
                buf = ""
            segs.append(("m", HOME))
            segs.append(("m", PATH))
            i = redacted_end(s, path_end(r, s, i + 1), before)
            continue
        if boundary and abs_start(s, i):
            end = path_end(r, s, i)
            tok = s[i:end]
            if keeps(r, tok):
                buf += tok
                i = end
            else:
                if buf:
                    segs.append(("t", buf))
                    buf = ""
                segs.append(("m", PATH))
                i = redacted_end(s, end, before)
            continue
        buf += s[i]
        i += 1
    if buf:
        segs.append(("t", buf))

    def split(segs, find):
        out = []
        for kind, t in segs:
            if kind == "m":
                out.append((kind, t))
                continue
            buf, i = "", 0
            while i < len(t):
                hit = find(t, i)
                if hit:
                    if buf:
                        out.append(("t", buf))
                        buf = ""
                    out.append(("m", hit[1]))
                    i += hit[0]
                else:
                    buf += t[i]
                    i += 1
            if buf:
                out.append(("t", buf))
        return out

    home = r["home"]

    def find_home(t, i):
        n = len(home)
        after_ok = not (i + n < len(t) and is_path_char(t[i + n]))
        if n and after_ok and starts_at(t, i, home, lambda a, b: path_eq(a, b, True)):
            return (n, HOME)
        return None

    segs = split(segs, find_home)
    for name, mark in r["names"]:

        def find_name(t, i, name=name, mark=mark):
            before_ok = i == 0 or not is_alnum(t[i - 1])
            after_ok = not (i + len(name) < len(t) and is_alnum(t[i + len(name)]))
            if before_ok and after_ok and starts_at(t, i, name, fold_eq):
                return (len(name), mark)
            return None

        segs = split(segs, find_name)
    return "".join(t for _, t in segs)


def fit_detail(s):
    return s if len(s) <= MAX_DETAIL else s[: MAX_DETAIL - 1] + "\u2026"


def redact_report(report, identity, install_dir):
    r = make_redactor(identity, install_dir)
    out = copy.deepcopy(report)
    out["tool_version"] = redact(r, out["tool_version"])
    snap = out["snapshot"]
    for item in PROBED:
        p = snap[item]
        if p["status"] == "unknown":
            p["detail"] = fit_detail(redact(r, p["detail"]))
    v = snap["os"].get("value")
    if snap["os"]["status"] == "known":
        v["family"], v["version"] = redact(r, v["family"]), redact(r, v["version"])
    if snap["arch"]["status"] == "known":
        snap["arch"]["value"] = redact(r, snap["arch"]["value"])
        assert len(snap["arch"]["value"]) <= 64
    if snap["cpu"]["status"] == "known":
        v = snap["cpu"]["value"]
        v["model"], v["instruction_set_level"] = redact(r, v["model"]), redact(r, v["instruction_set_level"])
    if snap["gpu"]["status"] == "known":
        v = snap["gpu"]["value"]
        for k in ["backend", "adapter", "driver_version", "device_type", "fallback_cause", "reason"]:
            if v[k] is not None:
                v[k] = redact(r, v[k])
    ass = out["assessment"]
    t = ass["tier"]
    if t["status"] == "unknown":
        t["detail"] = fit_detail(redact(r, t["detail"]))
    for p in ass["profiles"]:
        if p["name"] not in ("minimum", "recommended"):
            p["name"] = redact(r, p["name"])
            assert 1 <= len(p["name"]) <= 64
    ass["profiles"].sort(key=lambda p: p["name"])
    names = [p["name"] for p in ass["profiles"]]
    assert len(set(names)) == len(names), "redaction collision"
    out["redacted"] = True
    return out


# ------------------------------------------------------------------ redaction cases (hand-labelled)


def ident(user, hosts, home, domains=(), user_dirs=()):
    return {"domains": list(domains), "home": home, "hosts": list(hosts), "user": user, "user_dirs": list(user_dirs)}


ID_WIN = ident("jdoe", ["WS-QA-17.lab.example"], "C:\\Users\\jdoe")
ID_CORP = ident("jdoe", ["WS-QA-17", "ws-qa-17-long-name"], "C:\\Users\\jdoe", ["CORP", "CORP.EXAMPLE.COM"])
INSTALL_WIN = "C:\\Program Files\\Metrology Maven"
ID_SPACE = ident("Ann Lee", ["studio"], "C:\\Users\\Ann Lee")
ID_LINUX = ident("mtest", ["rig-04"], "/home/mtest")
INSTALL_LINUX = "/opt/metrology-maven"
ID_MAC = ident("kim", ["bench"], "/Users/kim")
INSTALL_MAC = "/Users/kim/Applications/Metrology Maven.app"
ID_NESTED = ident("lab", ["lab-pc"], "/home/lab")
ID_WORDS = ident("user", ["host"], "/home/user")
INSTALL_FID = "/opt/fid"
ID_JOSE = ident("Jos\u00e9", ["taller"], "/home/jose")
ID_SOFIA = ident("\u03a3\u03bf\u03c6\u03af\u03b1", ["lab"], "/home/sofia")
ID_ONEDRIVE = ident("jdoe", ["WS-QA-17"], "C:\\Users\\jdoe",
                    user_dirs=["C:\\Users\\jdoe\\OneDrive - Acme Corp", "C:\\Users\\jdoe\\OneDrive"])
ID_RENAMED = ident("jdoe", ["WS-QA-17"], "C:\\Users\\john.doe.CORP")

# (name, identity, install_dir, input, expected) -- expected written by hand from the rules.
REDACT_CASES = [
    ("user_host_path", ID_WIN, INSTALL_WIN,
     "probe failed for jdoe on WS-QA-17.lab.example reading C:\\Users\\jdoe\\data",
     "probe failed for <redacted:user> on <redacted:host> reading <redacted:path>"),
    ("host_first_label_any_case", ID_WIN, INSTALL_WIN, "host ws-qa-17 unreachable",
     "host <redacted:host> unreachable"),
    ("install_path_with_space_kept", ID_WIN, INSTALL_WIN,
     "loaded C:\\Program Files\\Metrology Maven\\bin\\fid.exe ok",
     "loaded C:\\Program Files\\Metrology Maven\\bin\\fid.exe ok"),
    ("install_path_case_and_slashes_kept", ID_WIN, INSTALL_WIN,
     "c:/program files/metrology maven/x.dll", "c:/program files/metrology maven/x.dll"),
    ("install_dir_itself_kept", ID_WIN, INSTALL_WIN, "C:\\Program Files\\Metrology Maven",
     "C:\\Program Files\\Metrology Maven"),
    ("install_escape_by_dotdot", ID_WIN, INSTALL_WIN,
     "C:\\Program Files\\Metrology Maven\\..\\..\\Users\\jdoe\\x", "<redacted:path>"),
    ("install_sibling_redacted_whole", ID_WIN, INSTALL_WIN, "C:\\Program Files\\Metrology Maven2\\x",
     "<redacted:path>"),
    ("swallow_stops_at_token_without_separator", ID_WIN, INSTALL_WIN,
     "read C:\\Data\\My Scans\\part 7 failed", "read <redacted:path> 7 failed"),
    ("swallow_onedrive_folder_with_dash", ID_WIN, INSTALL_WIN, "C:\\Users\\jdoe\\OneDrive - Acme Corp\\Docs\\x",
     "<redacted:path>"),
    ("onedrive_root_bare", ID_ONEDRIVE, INSTALL_WIN, "cannot access C:\\Users\\jdoe\\OneDrive - Acme Corp",
     "cannot access <redacted:path>"),
    ("onedrive_root_trailing_word", ID_ONEDRIVE, INSTALL_WIN, "cannot access C:\\Users\\jdoe\\OneDrive - Acme Corp now",
     "cannot access <redacted:path> now"),
    ("onedrive_root_trailing_period", ID_ONEDRIVE, INSTALL_WIN, "at C:\\Users\\jdoe\\OneDrive - Acme Corp.",
     "at <redacted:path>."),
    ("onedrive_root_quoted_forward_slashes", ID_ONEDRIVE, INSTALL_WIN, "dir (c:/users/jdoe/onedrive - acme corp) gone",
     "dir (<redacted:path>) gone"),
    ("onedrive_personal_root_trailing_word", ID_ONEDRIVE, INSTALL_WIN, "sync C:\\Users\\jdoe\\OneDrive paused",
     "sync <redacted:path> paused"),
    ("install_dir_with_trailing_period_kept", ID_WIN, INSTALL_WIN, "in C:\\Program Files\\Metrology Maven.",
     "in C:\\Program Files\\Metrology Maven."),
    ("swallow_other_home_with_two_spaces", ID_WIN, INSTALL_WIN, "C:\\Users\\Ann Marie Lee\\Documents\\x",
     "<redacted:path>"),
    ("swallow_client_folder_with_spaces", ID_WIN, INSTALL_WIN, "read D:\\Clients\\Acme Bank Audit\\part.stp failed",
     "read <redacted:path> failed"),
    ("swallow_over_redacts_later_separator_tokens", ID_WIN, INSTALL_WIN, "C:\\Temp\\a then 8/16 done",
     "<redacted:path> done"),
    ("swallow_stops_before_kept_install_path", ID_WIN, INSTALL_WIN,
     "copy C:\\Temp\\a C:\\Program Files\\Metrology Maven\\b", "copy <redacted:path> C:\\Program Files\\Metrology Maven\\b"),
    ("swallow_stays_on_its_line", ID_WIN, INSTALL_WIN, "C:\\Temp\\a\nb\\c", "<redacted:path>\nb\\c"),
    ("home_with_space_taken_whole", ID_SPACE, INSTALL_WIN, "cache at C:\\Users\\Ann Lee\\AppData\\cache.",
     "cache at <redacted:path>."),
    ("user_with_space", ID_SPACE, INSTALL_WIN, "owner Ann Lee, not Ann Leeds",
     "owner <redacted:user>, not Ann Leeds"),
    ("word_boundaries", ID_WIN, INSTALL_WIN, "jdoe2 and xjdoe and jdoe_x and JDOE",
     "jdoe2 and xjdoe and <redacted:user>_x and <redacted:user>"),
    ("unc_path", ID_WIN, INSTALL_WIN, "share \\\\fileserver\\scans\\jdoe ready",
     "share <redacted:path> ready"),
    ("rooted_path", ID_WIN, INSTALL_WIN, "\\Windows\\Temp\\jdoe.log", "<redacted:path>"),
    ("path_in_parentheses", ID_WIN, INSTALL_WIN, "(C:\\Users\\jdoe\\x.txt)", "(<redacted:path>)"),
    ("path_in_quotes_runs_to_quote", ID_WIN, INSTALL_WIN, "'C:\\Temp\\a b' and more", "'<redacted:path>' and more"),
    ("path_in_double_quotes_unclosed", ID_WIN, INSTALL_WIN, "open \"C:\\Temp\\my file", "open \"<redacted:path>"),
    ("drive_relative_is_not_absolute", ID_WIN, INSTALL_WIN, "C:data and D:", "C:data and D:"),
    ("tilde_home_posix", ID_LINUX, INSTALL_LINUX, "config at ~/.config/fid/gpu.toml now",
     "config at <redacted:home><redacted:path> now"),
    ("tilde_home_windows_with_space", ID_WIN, INSTALL_WIN, "saved to ~\\My Documents\\run.log",
     "saved to <redacted:home><redacted:path>"),
    ("tilde_inside_word_is_not_home", ID_LINUX, INSTALL_LINUX, "a~/b and v1.2~/x", "a~/b and v1.2~/x"),
    ("email_user_and_dns_domain", ID_CORP, INSTALL_WIN, "mail jdoe@corp.example.com today",
     "mail <redacted:user>@<redacted:host> today"),
    ("domain_backslash_user", ID_CORP, INSTALL_WIN, "logon CORP\\jdoe ok", "logon <redacted:host>\\<redacted:user> ok"),
    ("dns_host_name_beside_netbios", ID_CORP, INSTALL_WIN, "dns WS-QA-17-LONG-NAME and WS-QA-17",
     "dns <redacted:host> and <redacted:host>"),
    ("non_ascii_user", ID_JOSE, INSTALL_FID, "owner Jos\u00e9 (jos\u00e9)", "owner <redacted:user> (<redacted:user>)"),
    ("non_ascii_user_uppercase", ID_JOSE, INSTALL_FID, "OWNER JOS\u00c9", "OWNER <redacted:user>"),
    ("turkish_dotted_capital_i", ident("ilker", ["atolye"], "/home/ilker"), INSTALL_FID, "by \u0130LKER today",
     "by <redacted:user> today"),
    ("greek_user_uppercase", ID_SOFIA, INSTALL_FID, "\u03a3\u039f\u03a6\u038a\u0391 signed", "<redacted:user> signed"),
    ("forward_slash_home_in_free_text", ID_WIN, INSTALL_WIN, "set to C:/Users/jdoe.", "set to <redacted:path>."),
    ("forward_slash_home_after_equals", ID_MAC, INSTALL_MAC, "HOME=/Users/kim", "HOME=<redacted:path>"),
    ("file_url", ID_WIN, INSTALL_WIN, "open file:///C:/Users/jdoe/x.txt now", "open file:<redacted:path> now"),
    ("verbatim_prefix_redacted", ID_WIN, INSTALL_WIN, "at \\\\?\\C:\\Users\\jdoe\\x", "at <redacted:path>"),
    ("verbatim_prefix_install_kept", ID_WIN, INSTALL_WIN, "\\\\?\\C:\\Program Files\\Metrology Maven\\bin\\fid.exe",
     "\\\\?\\C:\\Program Files\\Metrology Maven\\bin\\fid.exe"),
    ("short_name_path", ID_WIN, INSTALL_WIN, "temp C:\\Users\\JDOE~1\\AppData\\Local\\Temp\\fid.log",
     "temp <redacted:path>"),
    ("short_name_in_text", ID_WIN, INSTALL_WIN, "profile JDOE~1", "profile <redacted:user>~1"),
    ("renamed_profile_name", ID_RENAMED, INSTALL_WIN, "profile john.doe.CORP loaded for jdoe",
     "profile <redacted:user> loaded for <redacted:user>"),
    ("renamed_profile_path", ID_RENAMED, INSTALL_WIN, "C:\\Users\\john.doe.CORP\\x", "<redacted:path>"),
    ("linux_install_kept_home_redacted", ID_LINUX, INSTALL_LINUX,
     "lib /opt/metrology-maven/lib/libfid.so; config /home/mtest/.config/fid (owner mtest)",
     "lib /opt/metrology-maven/lib/libfid.so; config <redacted:path> (owner <redacted:user>)"),
    ("linux_install_is_case_sensitive", ID_LINUX, INSTALL_LINUX, "/OPT/metrology-maven/x", "<redacted:path>"),
    ("linux_install_backslash_not_equal", ID_LINUX, INSTALL_LINUX, "\\opt\\metrology-maven\\x",
     "<redacted:path>"),
    ("per_user_install_home_redacted", ID_MAC, INSTALL_MAC,
     "running /Users/kim/Applications/Metrology Maven.app/Contents/MacOS/fid",
     "running <redacted:home>/Applications/Metrology Maven.app/Contents/MacOS/fid"),
    ("url_is_over_redacted", ID_WIN, INSTALL_WIN, "see https://example.org/x", "see https:<redacted:path>"),
    ("ratio_is_not_a_path", ID_WIN, INSTALL_WIN, "ratio 8/16 and a/b", "ratio 8/16 and a/b"),
    ("host_holding_user_longest_first", ID_NESTED, INSTALL_FID, "lab-pc by lab", "<redacted:host> by <redacted:user>"),
    ("placeholders_not_rescanned", ID_WORDS, INSTALL_FID, "user on host at /tmp/x",
     "<redacted:user> on <redacted:host> at <redacted:path>"),
    ("nothing_to_redact", ID_WIN, INSTALL_WIN, "Synthetic Discrete GPU 12G", "Synthetic Discrete GPU 12G"),
    ("empty", ID_WIN, INSTALL_WIN, "", ""),
]

# (name, identity, install_dir, error code, detail) -- hand-labelled refusals.
REDACTOR_ERRORS = [
    ("install_drive_root", ID_WIN, "C:\\", "invalid_install_dir", "is a root directory"),
    ("install_posix_root", ID_LINUX, "/", "invalid_install_dir", "is a root directory"),
    ("install_relative", ID_LINUX, "opt/fid", "invalid_install_dir", "not an absolute path"),
    ("install_with_dotdot", ID_LINUX, "/opt/../home", "invalid_install_dir", "has a `..` component"),
    ("install_holds_home", ID_WIN, "C:\\Users", "invalid_install_dir", "contains the home directory"),
    ("install_is_home", ID_WIN, "c:/users/JDOE/", "invalid_install_dir", "contains the home directory"),
    ("user_blank", dict(ID_WIN, user="  "), INSTALL_WIN, "identity_unavailable", "user"),
    ("hosts_empty", dict(ID_WIN, hosts=[]), INSTALL_WIN, "identity_unavailable", "host"),
    ("hosts_blank", dict(ID_WIN, hosts=["", " "]), INSTALL_WIN, "identity_unavailable", "host"),
    ("home_relative", dict(ID_WIN, home="jdoe"), INSTALL_WIN, "identity_unavailable", "home"),
]

# ------------------------------------------------------------------ reports


def read_json(path):
    with open(path, encoding="utf-8") as f:
        return json.load(f, object_pairs_hook=no_dup)


def no_dup(pairs):
    d = {}
    for k, v in pairs:
        if k in d:
            raise ValueError(f"duplicate key {k!r}")
        d[k] = v
    return d


def report(snapshot, assessment, phase="P1a", redacted=False, tool_version=TOOL_VERSION):
    return {
        "assessment": assessment,
        "phase": phase,
        "redacted": redacted,
        "schema_version": 1,
        "snapshot": snapshot,
        "tool_version": tool_version,
    }


def verdict(name, v, shortfalls=(), unknowns=(), reason=None):
    return {"name": name, "reason": reason, "shortfalls": list(shortfalls), "unknowns": list(unknowns), "verdict": v}


TIER1 = {"status": "known", "value": {"kind": "tier", "tier": "tier1"}}

# PLAT.DOCTOR.LOOKUP AC-1, labelled from that leaf's text: tier1, both meet, exit 0.
WIN11_ASSESSMENT = {
    "exit_code": 0,
    "outcome": "meets_recommended",
    "profiles": [verdict("minimum", "meets"), verdict("recommended", "meets")],
    "schema_version": 1,
    "tier": TIER1,
}


def win11_snapshot():
    return read_json(os.path.join(DOCTOR, "env-win11-discrete.json"))


def identifying_report():
    """AC-3 input: the Windows 11 snapshot with identifying strings and an unknown display."""
    s = win11_snapshot()
    s["display"] = {
        "status": "unknown",
        "reason": "error",
        "detail": "display probe failed for user jdoe on host WS-QA-17.lab.example reading "
        "C:\\Users\\jdoe\\data\\edid.bin via C:\\Program Files\\Metrology Maven\\bin\\edid.dll",
    }
    s["gpu"]["value"]["adapter"] = "Synthetic Discrete GPU 12G (WS-QA-17)"
    s["gpu"]["value"]["reason"] = "policy file C:\\Users\\jdoe\\AppData\\Roaming\\fid\\gpu.toml chosen by jdoe"
    # Display unknown: no shortfall, so both profiles are indeterminate listing display; the
    # tier is known, so the outcome is indeterminate, exit 4 (LOOKUP REQ-005 precedence).
    a = {
        "exit_code": 4,
        "outcome": "indeterminate",
        "profiles": [
            verdict("minimum", "indeterminate", unknowns=["display"]),
            verdict("recommended", "indeterminate", unknowns=["display"]),
            # A site profile named after the machine: redacted, and the profiles re-sorted.
            verdict("ws-qa-17 bench", "indeterminate", unknowns=["display"]),
        ],
        "schema_version": 1,
        "tier": TIER1,
    }
    return report(s, a)


IDENTIFYING_REDACTED_EXPECTED = {
    "profile_names": ["<redacted:host> bench", "minimum", "recommended"],
    "display_detail": "display probe failed for user <redacted:user> on host <redacted:host> reading "
    "<redacted:path> via C:\\Program Files\\Metrology Maven\\bin\\edid.dll",
    "gpu_adapter": "Synthetic Discrete GPU 12G (<redacted:host>)",
    "gpu_reason": "policy file <redacted:path> chosen by <redacted:user>",
}


def escape_report():
    """AC-6: ANSI escapes and a bidi override in strings (and in a detail)."""
    s = win11_snapshot()
    s["cpu"]["value"]["model"] = "Synthetic \x1b[31mRED\x1b[0m CPU \x1b]0;title\x07"
    s["gpu"]["value"]["adapter"] = "GPU \u202eevil\u202c name"
    s["ram"] = {"status": "unknown", "reason": "error", "detail": "meminfo \x1b[2Jcleared\x1b[0m"}
    a = {
        "exit_code": 4,
        "outcome": "indeterminate",
        "profiles": [
            verdict("minimum", "indeterminate", unknowns=["ram_gib"]),
            verdict("recommended", "indeterminate", unknowns=["ram_gib"]),
        ],
        "schema_version": 1,
        "tier": TIER1,
    }
    return report(s, a)


def lookup_reports():
    cases = read_json(os.path.join(DOCTOR, "lookup", "cases.json"))
    out = []
    for c in cases:
        rep = report(c["snapshot"], c["expected"], phase=c["phase"])
        if normalise(rep) != rep:
            raise SystemExit(f"lookup case {c['name']}: snapshot details are not normalised")
        out.append({"name": c["name"], "report": rep, "text": render(rep)})
    return out


# ------------------------------------------------------------------ invalid reports (hand-labelled)


def mutate(path, value=None, delete=False):
    def f(doc):
        node = doc
        for p in path[:-1]:
            node = node[p]
        if delete:
            del node[path[-1]]
        else:
            node[path[-1]] = value
        return doc

    return f


A = "assessment"
# (name, mutation, schema_valid, error code, pointer). schema_valid is whether the JSON Schema
# accepts it; the parser is stricter where the schema cannot say it (marked schema_valid True).
INVALID = [
    ("missing_tool_version", mutate(["tool_version"], delete=True), False, "schema_invalid", "/tool_version"),
    ("unknown_top_key", mutate(["outcome"], "meets_recommended"), False, "schema_invalid", ""),
    ("schema_version_string", mutate(["schema_version"], "1"), False, "schema_invalid", "/schema_version"),
    ("schema_version_fraction", mutate(["schema_version"], 1.5), False, "schema_invalid", "/schema_version"),
    ("schema_version_missing", mutate(["schema_version"], delete=True), False, "schema_invalid", "/schema_version"),
    ("schema_version_2", mutate(["schema_version"], 2), False, "unsupported_schema", ""),
    ("tool_version_empty", mutate(["tool_version"], ""), False, "schema_invalid", "/tool_version"),
    ("tool_version_blank", mutate(["tool_version"], "   "), False, "schema_invalid", "/tool_version"),
    ("tool_version_newline", mutate(["tool_version"], "0.1.0\n"), False, "schema_invalid", "/tool_version"),
    ("tool_version_65", mutate(["tool_version"], "9" * 65), False, "schema_invalid", "/tool_version"),
    ("phase_unknown", mutate(["phase"], "P9"), False, "schema_invalid", "/phase"),
    ("redacted_string", mutate(["redacted"], "false"), False, "schema_invalid", "/redacted"),
    ("snapshot_bad_reason", mutate(["snapshot", "ram"], {"status": "unknown", "reason": "nope", "detail": ""}),
     False, "schema_invalid", "/snapshot"),
    ("snapshot_extra_key", mutate(["snapshot", "host"], "x"), False, "schema_invalid", "/snapshot"),
    ("assessment_extra_key", mutate([A, "notes"], ""), False, "schema_invalid", "/assessment"),
    ("assessment_schema_2", mutate([A, "schema_version"], 2), False, "schema_invalid", "/assessment/schema_version"),
    ("outcome_unknown", mutate([A, "outcome"], "great"), False, "schema_invalid", "/assessment/outcome"),
    ("exit_code_mismatch", mutate([A, "exit_code"], 1), False, "schema_invalid", "/assessment/exit_code"),
    ("tier_kind_unknown", mutate([A, "tier", "value", "kind"], "gold"), False, "schema_invalid",
     "/assessment/tier/value/kind"),
    ("tier_planned_no_phase", mutate([A, "tier", "value"], {"kind": "planned", "tier": "tier1"}), False,
     "schema_invalid", "/assessment/tier/value/since_phase"),
    ("verdict_unknown", mutate([A, "profiles", 0, "verdict"], "ok"), False, "schema_invalid",
     "/assessment/profiles/0/verdict"),
    ("meets_with_unknown", mutate([A, "profiles", 0, "unknowns"], ["ram_gib"]), False, "schema_invalid",
     "/assessment/profiles/0/verdict"),
    ("below_without_shortfall", mutate([A, "profiles", 0, "verdict"], "below"), False, "schema_invalid",
     "/assessment/profiles/0/verdict"),
    ("reason_on_meets", mutate([A, "profiles", 0, "reason"], "no_profiles"), False, "schema_invalid",
     "/assessment/profiles/0/verdict"),
    ("shortfall_bad_field", mutate([A, "profiles", 0, "shortfalls"], [{"field": "colour", "found": 1, "required": 2}]),
     False, "schema_invalid", "/assessment/profiles/0/shortfalls/0/field"),
    ("shortfall_cores_fraction", mutate([A, "profiles", 0, "shortfalls"], [{"field": "cores", "found": 3.5, "required": 4}]),
     False, "schema_invalid", "/assessment/profiles/0/shortfalls/0/found"),
    ("unknown_field_twice", mutate([A, "profiles", 1, "unknowns"], ["ram_gib", "ram_gib"]), False, "schema_invalid",
     "/assessment/profiles/1/unknowns/1"),
    ("profile_name_empty", mutate([A, "profiles", 0, "name"], ""), False, "schema_invalid",
     "/assessment/profiles/0/name"),
    # Stricter than the schema: orders and the outcome rule.
    ("profiles_out_of_order", lambda d: (d[A]["profiles"].reverse(), d)[1], True, "schema_invalid",
     "/assessment/profiles/1/name"),
    ("unknowns_out_of_field_order", mutate([A, "profiles", 0], verdict("minimum", "indeterminate", unknowns=["display", "ram_gib"])),
     True, "schema_invalid", "/assessment/profiles/0/unknowns/1"),
    ("outcome_does_not_follow", lambda d: (d[A].update(outcome="meets_minimum", exit_code=1), d)[1], True,
     "schema_invalid", "/assessment/outcome"),
]


def invalid_cases():
    out = []
    for name, f, schema_valid, code, pointer in INVALID:
        base = report(win11_snapshot(), copy.deepcopy(WIN11_ASSESSMENT))
        doc = f(base)
        out.append({"code": code, "name": name, "pointer": pointer, "report": doc, "schema_valid": schema_valid})
    return out


# ------------------------------------------------------------------ JSON Schema

FORBIDDEN = "\\u0000-\\u001f\\u007f-\\u009f\\u061c\\u200b-\\u200f\\u2028-\\u202e\\u2060-\\u2064\\u2066-\\u2069\\ufeff"
LABEL = {"type": "string", "minLength": 1, "maxLength": 64, "pattern": "\\S", "not": {"pattern": "[" + FORBIDDEN + "]"}}
PHASES = ["P0", "P1a", "P1b", "P2", "P3", "P4", "later"]
TIERS = ["tier1", "tier2", "tier3"]
OUTCOMES = [("unsupported_platform", 3), ("below_minimum", 2), ("indeterminate", 4), ("meets_minimum", 1), ("meets_recommended", 0)]
FIELDS = ["cpu_level", "cores", "ram_gib", "gpu_class", "vram_gib", "storage_free_gib", "display"]
CPU_LEVELS = ["x86-64-v1", "x86-64-v2", "x86-64-v3", "x86-64-v4", "armv8.0", "armv8.2"]
GPU_CLASSES = ["software", "integrated", "discrete_entry", "discrete_mid"]
U32 = {"type": "integer", "minimum": 0, "maximum": 4294967295}
GIB = {"type": "number", "minimum": 0}
RES = {"type": "array", "prefixItems": [U32, U32], "minItems": 2, "maxItems": 2, "items": False}


def obj(props, required=None):
    return {
        "type": "object",
        "additionalProperties": False,
        "required": sorted(props) if required is None else required,
        "properties": props,
    }


def shortfall(fields, required, found):
    return obj({"field": {"enum": fields}, "required": required, "found": found})


def schema():
    with open(os.path.join(ROOT, "platform", "env-snapshot.schema.json"), encoding="utf-8") as f:
        snap = json.load(f, object_pairs_hook=no_dup)
    snap.pop("$schema", None)
    snap["$id"] = "urn:metrology-maven:env-snapshot:1"
    tier_known = obj(
        {
            "status": {"const": "known"},
            "value": {
                "oneOf": [
                    obj({"kind": {"const": "tier"}, "tier": {"enum": TIERS}}),
                    obj({"kind": {"const": "planned"}, "since_phase": {"enum": PHASES}, "tier": {"enum": TIERS}}),
                    obj({"kind": {"const": "not_listed"}}),
                ]
            },
        }
    )
    empty = {"type": "array", "maxItems": 0}
    nonempty = {"type": "array", "minItems": 1}
    profile = obj(
        {
            "name": LABEL,
            "reason": {"enum": [None, "no_profiles", "missing_profile"]},
            "shortfalls": {"type": "array", "maxItems": len(FIELDS), "items": {"$ref": "#/$defs/shortfall"}},
            "unknowns": {"type": "array", "maxItems": len(FIELDS), "uniqueItems": True, "items": {"enum": FIELDS}},
            "verdict": {"enum": ["meets", "below", "indeterminate"]},
        }
    )
    profile["oneOf"] = [
        {"properties": {"verdict": {"const": "meets"}, "reason": {"const": None}, "shortfalls": empty, "unknowns": empty}},
        {"properties": {"verdict": {"const": "below"}, "reason": {"const": None}, "shortfalls": nonempty}},
        {"properties": {"verdict": {"const": "indeterminate"}, "reason": {"const": None}, "shortfalls": empty, "unknowns": nonempty}},
        {"properties": {"verdict": {"const": "indeterminate"}, "reason": {"enum": ["no_profiles", "missing_profile"]}, "shortfalls": empty, "unknowns": empty}},
    ]
    assessment = obj(
        {
            "exit_code": {"enum": [c for _, c in OUTCOMES]},
            "outcome": {"enum": [o for o, _ in OUTCOMES]},
            "profiles": {"type": "array", "maxItems": 64, "items": {"$ref": "#/$defs/profile_verdict"}},
            "schema_version": {"const": 1},
            "tier": {"oneOf": [{"$ref": "urn:metrology-maven:env-snapshot:1#/$defs/unknown"}, tier_known]},
        }
    )
    assessment["oneOf"] = [{"properties": {"outcome": {"const": o}, "exit_code": {"const": c}}} for o, c in OUTCOMES]
    root = obj(
        {
            "assessment": {"$ref": "#/$defs/assessment"},
            "phase": {"enum": PHASES},
            "redacted": {"type": "boolean"},
            "schema_version": {"const": 1},
            "snapshot": {"$ref": "#/$defs/snapshot"},
            "tool_version": LABEL,
        }
    )
    out = {
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "$id": "urn:metrology-maven:doctor-report:1",
        "title": "Metrology Maven doctor report, schema_version 1 (PLAT.DOCTOR.REPORT)",
        "$comment": "Written by fixtures/plat/doctor/report/make_report.py; do not edit by hand.",
    }
    out.update(root)
    out["$defs"] = {
        "assessment": assessment,
        "profile_verdict": profile,
        "shortfall": {
            "oneOf": [
                shortfall(["cpu_level"], {"enum": CPU_LEVELS}, {"enum": CPU_LEVELS}),
                shortfall(["cores"], U32, U32),
                shortfall(["ram_gib", "vram_gib", "storage_free_gib"], U32, GIB),
                shortfall(["gpu_class"], {"enum": GPU_CLASSES}, {"enum": GPU_CLASSES}),
                shortfall(["display"], RES, RES),
            ]
        },
        "snapshot": snap,
    }
    return out


# ------------------------------------------------------------------ self-checks and output


def self_check():
    for name, ident, inst, inp, expected in REDACT_CASES:
        got = redact(make_redactor(ident, inst), inp)
        if got != expected:
            raise SystemExit(f"redact case {name}: oracle gives {got!r}, hand label {expected!r}")
    for name, ident, inst, code, detail in REDACTOR_ERRORS:
        try:
            make_redactor(ident, inst)
        except RedactError as e:
            if (e.code, e.detail) != (code, detail):
                raise SystemExit(f"redactor error {name}: {(e.code, e.detail)} != {(code, detail)}")
        else:
            raise SystemExit(f"redactor error {name}: accepted")
    # The Windows 11 report has nothing identifying for ID_WIN.
    base = report(win11_snapshot(), WIN11_ASSESSMENT)
    red = redact_report(base, ID_WIN, INSTALL_WIN)
    if dict(red, redacted=False) != base:
        raise SystemExit("win11 report changed under redaction")
    ident = redact_report(identifying_report(), ID_WIN, INSTALL_WIN)
    exp = IDENTIFYING_REDACTED_EXPECTED
    got = {
        "display_detail": ident["snapshot"]["display"]["detail"],
        "gpu_adapter": ident["snapshot"]["gpu"]["value"]["adapter"],
        "gpu_reason": ident["snapshot"]["gpu"]["value"]["reason"],
        "profile_names": [p["name"] for p in ident["assessment"]["profiles"]],
    }
    if got != exp:
        raise SystemExit(f"identifying report: {got!r} != hand label {exp!r}")
    for bad in ["jdoe", "ws-qa-17", "c:\\users\\jdoe\\data"]:
        if bad in canon(ident).lower().replace("\\\\", "\\"):
            raise SystemExit(f"identifying report still holds {bad!r}")
    if js_num(0.1) != "0.1" or js_num(1e-7) != "1e-7" or js_num(1e21) != "1e+21" or js_num(31.84) != "31.84" \
            or js_num(0.000001) != "0.000001" or js_num(5.99) != "5.99" or js_num(12.0) != "12":
        raise SystemExit("js_num self-check failed")


def outputs():
    win11 = redact_report(report(win11_snapshot(), WIN11_ASSESSMENT), ID_WIN, INSTALL_WIN)
    schema99 = dict(win11, schema_version=99)
    esc_rep = escape_report()
    ident = identifying_report()
    ident_red = redact_report(ident, ID_WIN, INSTALL_WIN)
    redact_cases = {
        "errors": [
            {"code": code, "detail": detail, "identity": ident_, "install_dir": inst, "name": name}
            for name, ident_, inst, code, detail in REDACTOR_ERRORS
        ],
        "identifying": {"identity": ID_WIN, "install_dir": INSTALL_WIN},
        "strings": [
            {"expected": exp, "identity": ident_, "input": inp, "install_dir": inst, "name": name}
            for name, ident_, inst, inp, exp in REDACT_CASES
        ],
    }
    j = lambda v: canon_bytes(v)  # noqa: E731
    return {
        os.path.join(DOCTOR, "report-win11.json"): j(win11),
        os.path.join(DOCTOR, "report-win11.txt"): render(win11).encode("utf-8"),
        os.path.join(DOCTOR, "report-schema99.json"): j(schema99),
        # ASCII-only JSON (\u escapes), so the file itself holds no raw escape or bidi character.
        os.path.join(DOCTOR, "report-escape.json"): json.dumps(esc_rep, sort_keys=True, separators=(",", ":"), ensure_ascii=True).encode("ascii"),
        os.path.join(DOCTOR, "report-escape.txt"): render(esc_rep).encode("utf-8"),
        os.path.join(HERE, "identifying.json"): j(ident),
        os.path.join(HERE, "identifying.redacted.json"): j(ident_red),
        os.path.join(HERE, "identifying.redacted.txt"): render(ident_red).encode("utf-8"),
        os.path.join(HERE, "redact_cases.json"): j(redact_cases),
        os.path.join(HERE, "cases.json"): j(lookup_reports()),
        os.path.join(HERE, "invalid_cases.json"): j(invalid_cases()),
        os.path.join(ROOT, "platform", "doctor-report.schema.json"): (json.dumps(schema(), indent=2, ensure_ascii=False) + "\n").encode("utf-8"),
    }


def main():
    check = "--check" in sys.argv[1:]
    self_check()
    bad = 0
    for path, data in outputs().items():
        rel = os.path.relpath(path, ROOT)
        if check:
            try:
                with open(path, "rb") as f:
                    same = f.read() == data
            except FileNotFoundError:
                same = False
            if not same:
                print(f"{rel}: differs from the oracle output", file=sys.stderr)
                bad += 1
        else:
            with open(path, "wb") as f:
                f.write(data)
            print(f"wrote {rel}")
    if bad:
        sys.exit(1)


if __name__ == "__main__":
    main()
