#!/usr/bin/env python3
"""Generate the KRN.MAN.CARD fixtures (CC-BY-4.0 project data).

- fixtures/man/card-ids-v1.txt: 50 valid and 50 invalid algorithm IDs (AC-1)
- fixtures/man/cards-v1/*.toml: 3 valid cards, 3 cards each missing one field (AC-2),
  and one card quoting a 40-word passage next to a standard reference (AC-4)
Run:  python fixtures/man/make_fixtures.py
"""

from pathlib import Path

HERE = Path(__file__).parent
modules = ["fit", "gdt", "unc", "geo", "aln"]
features = ["cylinder", "plane", "circle_2d", "sphere", "flatness"]
methods = ["ls", "mz", "lsq_weighted", "chebyshev", "gauss2"]
valid = []
for i in range(50):
    m, f, me = modules[i % 5], features[(i // 5) % 5], methods[(i * 3) % 5]
    v = (i % 4, (i * 7) % 11, 0 if i % 3 else 10 + i)
    valid.append(f"{m}.{f}.{me}@{v[0]}.{v[1]}.{v[2]}")
valid[0] = "fit.cylinder.ls@1.0.0"
valid[1] = "a.b.c@0.0.0"
valid[2] = "a1.b_2.c_3_@4294967295.0.1"
invalid = [
    "Fit.cylinder.ls@1.0.0", "fit.cylinder.ls@1.0", "fit.cylinder@1.0.0", "fit.cylinder.ls@01.0.0",
    "fit.cylinder.ls@1.00.0", "fit.cylinder.ls@1.0.00", "fit.cylinder.ls", "fit.cylinder.ls@", "@1.0.0",
    "fit..ls@1.0.0", ".fit.cylinder.ls@1.0.0", "fit.cylinder.ls.@1.0.0", "fit.cylinder.ls@1.0.0.0",
    "fit.cylinder.ls@1..0", "fit.cylinder.ls@-1.0.0", "fit.cylinder.ls@1.0.x", "fit.cylinder.ls@4294967296.0.0",
    "1fit.cylinder.ls@1.0.0", "_fit.cylinder.ls@1.0.0", "fit.Cylinder.ls@1.0.0", "fit.cylinder.LS@1.0.0",
    "fit-x.cylinder.ls@1.0.0", "fit.cyl inder.ls@1.0.0", "fit.cylinder.ls @1.0.0", "fit.cylinder.ls@ 1.0.0",
    " fit.cylinder.ls@1.0.0", "fit.cylinder.ls@1.0.0 ", "fit.cylinder.ls@@1.0.0", "fit.cylinder.ls@1.0.0@1",
    "fit.cylinder.ls@+1.0.0", "fit.cylinder.ls@1.0.0-rc1", "fit.cylinder.ls@v1.0.0", "fit/cylinder/ls@1.0.0",
    "fit.cylinder.ls#1.0.0", "fit.cylinder.ls@1,0,0", "fit.cylinder.lś@1.0.0", "fit.cylinder.ls@١.0.0",
    "", "@", "...@..", "fit.cylinder.ls@1.0.", "fit.cylinder.ls@.0.0", "fit.cylinder.ls@1.0.0\t",
    "fit.cylinder.ls.extra@1.0.0", "fit@1.0.0", "fit.cylinder.ls@00.0.0", "fit.cylinder.ls@1.01.0",
    "FIT.CYLINDER.LS@1.0.0", "fit.cylinder.ls@1.0.0.", "fit.cylinder.ls@1e3.0.0",
]
assert len(valid) == 50 and len(set(valid)) == 50, len(set(valid))
assert len(invalid) == 50 and len(set(invalid)) == 50, len(set(invalid))
lines = ["# KRN.MAN.CARD AC-1. Format: <valid|invalid><TAB><id>. CC-BY-4.0."]
lines += [f"valid\t{x}" for x in valid] + [f"invalid\t{x}" for x in invalid]
(HERE / "card-ids-v1.txt").write_text("\n".join(lines) + "\n", encoding="utf-8", newline="\n")

GOOD = {
    "valid-fit-cylinder-ls": dict(
        id="fit.cylinder.ls@1.0.0",
        title="Least-squares cylinder",
        method="Gauss-Newton on the orthogonal distance to the cylinder surface, started from a PCA axis estimate.",
        applicability="At least 5 points spread over more than 30 degrees of arc; not for very short cylinders.",
        references=["ISO 10360-6:2001, clause 5 (test method for the software)",
                    "Forbes, A. B. (1989). Least-squares best-fit geometric elements. NPL Report DITC 140/89."],
        determinism="D0",
        failure_modes=["Points on a narrow arc make the radius ill-conditioned.",
                       "Two axis directions fit equally well when the points lie on a ring."],
        validation="Pending: NIST ATS cylinder datasets (see UNC.RDL.EXT.NIST)."),
    "valid-gdt-flatness-mz": dict(
        id="gdt.flatness.mz@0.1.0",
        title="Minimum-zone flatness",
        method="Exact minimum zone between two parallel planes via the convex-hull antipodal pair search.",
        applicability="Any planar point set with at least 4 non-collinear points.",
        references=["ASME Y14.5-2018, clause 5.4.4 (flatness)"],
        determinism="D0",
        failure_modes=["Collinear points give an undefined plane."],
        validation="Pending: reference values from the PTB softgauges (see UNC.RDL.EXT.PTBSG)."),
    "valid-unc-mc-sample": dict(
        id="unc.mc.sample@1.2.3",
        title="Monte Carlo uncertainty propagation",
        method="Counter-based random streams, fixed sample count, fixed-order reductions.",
        applicability="Models with finite variance inputs; outputs reported with a coverage interval.",
        references=["JCGM 101:2008, clause 7 (propagation of distributions)"],
        determinism="D1",
        failure_modes=["Heavy-tailed inputs need more samples than the default."],
        validation="Pending: JCGM 101 worked examples reproduced in own words (see UNC.RDL.EXT.JCGM)."),
}


def toml(card):
    out = ["# Spec card. Cite designators and clause numbers only; never paste standards text."]
    for k, v in card.items():
        if isinstance(v, list):
            out.append(f"{k} = [")
            out += [f'  "{x}",' for x in v]
            out.append("]")
        else:
            out.append(f'{k} = "{v}"')
    return "\n".join(out) + "\n"


d = HERE / "cards-v1"
d.mkdir(exist_ok=True)
for name, card in GOOD.items():
    (d / f"{name}.toml").write_text(toml(card), encoding="utf-8", newline="\n")
base = GOOD["valid-fit-cylinder-ls"]
for field in ["applicability", "references", "validation"]:
    c = {k: v for k, v in base.items() if k != field}
    (d / f"missing-{field}.toml").write_text(toml(c), encoding="utf-8", newline="\n")
quote = " ".join(["word"] * 40)
c = dict(base, references=[f'ISO 1101:2017, clause 18 says \\"{quote}\\"'])
(d / "standards-text-quote.toml").write_text(toml(c), encoding="utf-8", newline="\n")
print("ok")
