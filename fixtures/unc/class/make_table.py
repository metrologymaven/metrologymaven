#!/usr/bin/env python3
"""Independent reference for UNC.UEC.CLASS (rule set uec-assign@1); stdlib only, CC-BY-4.0 data.

Writes fixtures/unc/class/table.csv: one row per input case with the expected class, UEC-0
reason, downgrade, labels or error. The expected columns come from the rules written out in
spec/unc/uec/class/NODE.md (Design), re-implemented here in Python without looking at, or
running, the Rust code. A few rows are also hand-labelled (HAND below) and the script asserts
that the oracle agrees with every hand label.

Columns
  case                 unique row name
  basis                wire name (none, declared, spec_derived, modelled_lpu, modelled_mcm,
                       iso15530_3, iso15530_4) or an unknown string
  model_bound          true / false
  instruments_bound    true / false
  contributors         "-" or ";"-separated "<input_id>:<claim>:<record>[:<label>]", in input order
                         claim:  dcc qualification type_a declared im_iemps im_spec
                                 none (no source) im (instrument model without origin)
                                 dcc_iemps (dcc carrying an origin)
                         record: what the record store holds for the claim's digest:
                                 same (the claim itself), absent, or a claim word
                                 (dcc qualification type_a declared im_iemps im_spec)
                         label:  record label (default: the input_id); contributors with the
                                 same label cite one shared record, whose kind they must agree on
  validation           "-" (no record reference), ok (store holds a validation record),
                       absent (unknown digest), wrong (the digest is a dcc source record)
  class                UEC-0..UEC-3 or "-" on error
  uec0_reason          u_missing / model_missing / instrument_missing or "-"
  downgrade            unsourced_contributor / uncharacterized_contributor /
                       unverified_source / not_validated or "-"
  downgrade_input      the named contributor or "-"
  claimed, declared_input, uncharacterized_input   true / false, or "-" on error
  error                "-" or "<code>" or "<code>:<field>"

Digests (computed by the test, not stored): a contributor's claimed source is
sha256("source:" + label); the validation record is sha256("validation:" + case).

Usage: make_table.py [--check]   (--check compares with the file on disk, writes nothing)
"""

import pathlib
import sys

HERE = pathlib.Path(__file__).resolve().parent
OUT = HERE / "table.csv"

BASES = ["none", "declared", "spec_derived", "modelled_lpu", "modelled_mcm", "iso15530_3", "iso15530_4"]
CLAIMED = {"declared", "spec_derived"}
MODELLED = {"modelled_lpu", "modelled_mcm"}
SUBST = {"iso15530_3", "iso15530_4"}
CHARACTERIZED = {"dcc", "qualification", "type_a"}

HEADER = [
    "case", "basis", "model_bound", "instruments_bound", "contributors", "validation",
    "class", "uec0_reason", "downgrade", "downgrade_input",
    "claimed", "declared_input", "uncharacterized_input", "error",
]


def token_ok(s):
    b = s.encode("utf-8")
    return 1 <= len(b) <= 256 and all(0x21 <= c <= 0x7E and c not in (0x22, 0x5C) for c in b)


def parse_contributors(text):
    if text == "-":
        return []
    out = []
    shared = {}
    for item in text.split(";"):
        fields = item.split(":")
        ident, claim, record = fields[:3]
        label = fields[3] if len(fields) == 4 else ident
        assert len(fields) in (3, 4), item
        if claim != "none" and record != "absent":
            rec = claim if record == "same" else record
            assert shared.setdefault(label, rec) == rec, ("shared record with two kinds", item)
        out.append((ident, claim, record))
    return out


def tf(x):
    return "true" if x else "false"


def oracle(basis, model_bound, instruments_bound, contributors, validation):
    """Returns (class, uec0_reason, downgrade, downgrade_input, claimed, declared, unchar) or ("error", code)."""
    if basis not in BASES:
        return ("error", "unknown_basis")
    cs = parse_contributors(contributors)
    for ident, _, _ in cs:
        if not token_ok(ident):
            return ("error", "invalid_input:input_id")
    cs = sorted(cs, key=lambda c: c[0].encode("utf-8"))
    ids = [c[0] for c in cs]
    if len(set(ids)) != len(ids):
        return ("error", "duplicate_input")
    for _, claim, _ in cs:
        if claim in ("im", "dcc_iemps"):
            return ("error", "inconsistent_input:origin")

    if basis == "none":
        if cs:
            return ("error", "inconsistent_input:contributors")
        if validation != "-":
            return ("error", "inconsistent_input:validation_record")
        if not model_bound:
            reason = "model_missing"
        elif not instruments_bound:
            reason = "instrument_missing"
        else:
            reason = "u_missing"
        return ("UEC-0", reason, "-", "-", False, False, False)

    if basis in CLAIMED:
        if validation != "-":
            return ("error", "inconsistent_input:validation_record")
    else:
        if not model_bound:
            return ("error", "inconsistent_input:model_bound")
        if not instruments_bound:
            return ("error", "inconsistent_input:instruments_bound")
        if not cs:
            return ("error", "empty_model")
        if basis in MODELLED and validation != "-":
            return ("error", "inconsistent_input:validation_record")

    # Verify each claim against its record. Only a characterized claim can be confirmed into
    # a characterized source; other claims report their own weakness.
    declared = False
    unchar = False
    first = None
    for ident, claim, record in cs:
        rec = claim if record == "same" else record
        if claim == "declared" or (claim != "none" and rec == "declared"):
            declared = True
        if claim == "none" or claim == "declared" or claim == "im_spec":
            status = "unsourced_contributor"
        elif claim == "im_iemps":
            status = "uncharacterized_contributor"
        elif rec == claim:
            status = None
        else:
            status = "unverified_source"
        if status is not None:
            unchar = True
            if first is None:
                first = (status, ident)

    claimed = basis in CLAIMED
    if claimed:
        return ("UEC-1", "-", "-", "-", True, declared, unchar)
    if first is not None:
        return ("UEC-1", "-", first[0], first[1], False, declared, unchar)
    if basis in SUBST:
        if validation == "ok":
            return ("UEC-3", "-", "-", "-", False, declared, unchar)
        return ("UEC-2", "-", "not_validated", "-", False, declared, unchar)
    return ("UEC-2", "-", "-", "-", False, declared, unchar)


def cases():
    rows = []

    def add(case, basis, mb, ib, contributors, validation):
        rows.append((case, basis, mb, ib, contributors, validation))

    # UEC-0 (REQ-UNC.UEC.CLASS-001)
    add("none_no_model", "none", False, False, "-", "-")
    add("none_no_model_instruments_bound", "none", False, True, "-", "-")
    add("none_no_instrument", "none", True, False, "-", "-")
    add("none_u_missing", "none", True, True, "-", "-")
    add("none_with_contributors", "none", True, True, "a:dcc:same", "-")
    add("none_with_validation", "none", True, True, "-", "ok")

    # Claimed bases (REQ-UNC.UEC.CLASS-005): always UEC-1, never promoted.
    for b in ("declared", "spec_derived"):
        add(f"{b}_bare", b, True, True, "-", "-")
        add(f"{b}_dcc_source", b, True, True, "a:dcc:same", "-")
        add(f"{b}_declared_source", b, True, True, "a:declared:same", "-")
        add(f"{b}_im_spec_source", b, True, True, "a:im_spec:same", "-")
        add(f"{b}_model_unbound", b, False, False, "-", "-")
        add(f"{b}_with_validation", b, True, True, "-", "ok")

    # Evaluated bases: every source-kind mix (REQ-UNC.UEC.CLASS-002/-003/-004).
    mixes = [
        ("all_dcc", "a:dcc:same;b:dcc:same;c:dcc:same"),
        ("mixed_characterized", "a:dcc:same;b:qualification:same;c:type_a:same"),
        ("one_declared", "a:dcc:same;b:declared:same;c:dcc:same"),
        ("one_unsourced", "a:dcc:same;b:none:absent;c:dcc:same"),
        ("one_im_spec", "a:dcc:same;b:im_spec:same;c:dcc:same"),
        ("one_im_iemps", "a:dcc:same;b:im_iemps:same;c:dcc:same"),
        ("one_record_absent", "a:dcc:same;b:dcc:absent;c:dcc:same"),
        ("one_record_other_kind", "a:dcc:same;b:qualification:dcc;c:dcc:same"),
        ("declared_claim_dcc_record", "a:dcc:same;b:declared:dcc;c:dcc:same"),
        ("dcc_claim_declared_record", "a:dcc:same;b:dcc:declared;c:dcc:same"),
        ("iemps_claim_spec_record", "a:dcc:same;b:im_iemps:im_spec"),
        ("offenders_out_of_order", "c:declared:same;a:dcc:same;b:im_iemps:same"),
        ("empty", "-"),
    ]
    for b in ("modelled_lpu", "modelled_mcm", "iso15530_3", "iso15530_4"):
        v = "ok" if b in SUBST else "-"
        for name, mix in mixes:
            add(f"{b}_{name}", b, True, True, mix, v)
        add(f"{b}_model_unbound", b, False, True, "a:dcc:same", v)
        add(f"{b}_instruments_unbound", b, True, False, "a:dcc:same", v)
    for b in ("iso15530_3", "iso15530_4"):
        add(f"{b}_validation_missing", b, True, True, "a:dcc:same;b:type_a:same", "-")
        add(f"{b}_validation_absent", b, True, True, "a:dcc:same;b:type_a:same", "absent")
        add(f"{b}_validation_wrong_kind", b, True, True, "a:dcc:same;b:type_a:same", "wrong")
        add(f"{b}_declared_no_validation", b, True, True, "a:dcc:same;b:declared:same", "-")
    for b in ("modelled_lpu", "modelled_mcm"):
        add(f"{b}_with_validation", b, True, True, "a:dcc:same", "ok")

    # Two contributors citing one shared record: same kind, and one claim of another kind.
    for b in ("modelled_lpu", "iso15530_3"):
        v = "ok" if b in SUBST else "-"
        add(f"{b}_shared_record_same_kind", b, True, True, "a:dcc:same:s;b:dcc:same:s", v)
        add(f"{b}_shared_record_other_kind", b, True, True, "a:dcc:same:s;b:type_a:dcc:s", v)

    # Malformed or ambiguous input.
    add("unknown_basis_modelled", "modelled", True, True, "a:dcc:same", "-")
    add("unknown_basis_case", "MODELLED_MCM", True, True, "a:dcc:same", "-")
    add("duplicate_input", "modelled_mcm", True, True, "a:dcc:absent;b:dcc:same;a:type_a:absent", "-")
    add("instrument_model_without_origin", "modelled_lpu", True, True, "a:dcc:same;b:im:absent", "-")
    add("origin_on_dcc", "modelled_lpu", True, True, "a:dcc_iemps:absent", "-")
    add("input_id_not_token", "modelled_lpu", True, True, "a:dcc:same;b\"x:dcc:same", "-")

    # Named acceptance rows.
    add("mcm_one_declared", "modelled_mcm", True, True,
        "c1:dcc:same;c2:dcc:same;c3:declared:same;c4:dcc:same;c5:dcc:same", "-")
    add("subst_no_validation", "iso15530_3", True, True, "a:dcc:same;b:type_a:same", "-")
    add("modelled_empty", "modelled_mcm", True, True, "-", "-")
    return rows


# Hand labels from the spec text (class, uec0_reason, downgrade, downgrade_input) or error.
HAND = {
    "none_no_model": ("UEC-0", "model_missing", "-", "-"),
    "none_no_instrument": ("UEC-0", "instrument_missing", "-", "-"),
    "none_u_missing": ("UEC-0", "u_missing", "-", "-"),
    "declared_dcc_source": ("UEC-1", "-", "-", "-"),
    "mcm_one_declared": ("UEC-1", "-", "unsourced_contributor", "c3"),
    "subst_no_validation": ("UEC-2", "-", "not_validated", "-"),
    "iso15530_4_all_dcc": ("UEC-3", "-", "-", "-"),
    "modelled_lpu_mixed_characterized": ("UEC-2", "-", "-", "-"),
    "modelled_lpu_one_im_spec": ("UEC-1", "-", "unsourced_contributor", "b"),
    "modelled_lpu_one_im_iemps": ("UEC-1", "-", "uncharacterized_contributor", "b"),
    "modelled_lpu_one_record_absent": ("UEC-1", "-", "unverified_source", "b"),
    "modelled_mcm_declared_claim_dcc_record": ("UEC-1", "-", "unsourced_contributor", "b"),
    "iso15530_3_offenders_out_of_order": ("UEC-1", "-", "uncharacterized_contributor", "b"),
    "iso15530_3_declared_no_validation": ("UEC-1", "-", "unsourced_contributor", "b"),
    "modelled_empty": "empty_model",
    "unknown_basis_modelled": "unknown_basis",
    "duplicate_input": "duplicate_input",
    "modelled_lpu_shared_record_same_kind": ("UEC-2", "-", "-", "-"),
    "modelled_lpu_shared_record_other_kind": ("UEC-1", "-", "unverified_source", "b"),
    "iso15530_3_shared_record_same_kind": ("UEC-3", "-", "-", "-"),
}


def render():
    lines = [",".join(HEADER)]
    seen = set()
    for case, basis, mb, ib, contributors, validation in cases():
        assert case not in seen, case
        seen.add(case)
        assert "," not in contributors and "\n" not in contributors
        r = oracle(basis, mb, ib, contributors, validation)
        if r[0] == "error":
            expect = ["-"] * 7 + [r[1]]
            label = r[1]
        else:
            cls, reason, down, down_id, claimed, declared, unchar = r
            expect = [cls, reason, down, down_id, tf(claimed), tf(declared), tf(unchar), "-"]
            label = (cls, reason, down, down_id)
        if case in HAND:
            assert HAND[case] == label, (case, HAND[case], label)
        lines.append(",".join([case, basis, tf(mb), tf(ib), contributors, validation] + expect))
    assert set(HAND) <= seen
    return ("\n".join(lines) + "\n").encode("utf-8")


def main():
    data = render()
    n = data.count(b"\n") - 1
    if "--check" in sys.argv[1:]:
        if not OUT.exists() or OUT.read_bytes() != data:
            print("table.csv is stale; run make_table.py", file=sys.stderr)
            return 1
        print(f"ok: {OUT.name} ({n} rows)")
        return 0
    OUT.write_bytes(data)
    print(f"wrote {OUT.name} ({n} rows)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
