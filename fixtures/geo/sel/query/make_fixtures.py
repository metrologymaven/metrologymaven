#!/usr/bin/env python3
"""Deterministic fixture generator for GEO.SEL.QUERY.GRAMMAR (CC-BY-4.0).

Generates:
  - fixtures/geo/sel/query/valid_v1.json (60 valid queries with canonical bytes & sha256 digests)
  - fixtures/geo/sel/query/equivalent_pairs.json (40 equivalent + 20 non-equivalent pairs)
  - fixtures/geo/sel/query/invalid_v1.json (80 malformed queries with expected JSON pointers)
"""

from __future__ import annotations

import argparse
from fractions import Fraction
import hashlib
import json
import math
import pathlib
import struct
import sys

HERE = pathlib.Path(__file__).resolve().parent

U1 = "00000000-0000-7000-8000-000000000001"
U2 = "00000000-0000-7000-8000-000000000002"
U3 = "00000000-0000-7000-8000-000000000003"
U4 = "00000000-0000-7000-8000-000000000004"

# Exact conversion factors for simple decimal units (matching fid-geo-frame::units::to_si).
LENGTH_FACTORS: dict[str, Fraction] = {
    "m": Fraction(1, 1),
    "mm": Fraction(1, 1000),
    "um": Fraction(1, 1_000_000),
    "µm": Fraction(1, 1_000_000),
    "in": Fraction(254, 10_000),
}

# For angles: rad and mrad are exact rational; deg uses exact high-precision pi/180
# (rounded once to IEEE 754 binary64, identical to fid-geo-frame::units).
PI_EXACT = Fraction(
    31415926535897932384626433832795028841971693993751058209749445923078164062862089986280348253421170679,
    10**100,
)
ANGLE_FACTORS: dict[str, Fraction] = {
    "rad": Fraction(1, 1),
    "deg": PI_EXACT / 180,
    "arcmin": PI_EXACT / 10_800,
    "arcsec": PI_EXACT / 648_000,
    "gon": PI_EXACT / 200,
}


def qty_si_hex(text: str, unit: str) -> str:
    fac = LENGTH_FACTORS.get(unit) or ANGLE_FACTORS.get(unit)
    assert fac is not None, f"unknown unit {unit}"
    val = float(Fraction(text) * fac)
    if val == 0.0:
        val = 0.0
    bits = struct.unpack(">Q", struct.pack(">d", val))[0]
    return f"0x{bits:016x}"


def jcs_str(s: str) -> str:
    return json.dumps(s, ensure_ascii=False)


def jcs_num(x: float | int) -> str:
    if isinstance(x, int) and not isinstance(x, bool):
        return str(x)
    v = float(x)
    if v == 0.0:
        return "0"
    if v.is_integer() and abs(v) < 1e21:
        return str(int(v))
    s = repr(v)
    if "e" in s or "E" in s:
        s = s.replace("E", "e")
        mant, exp = s.split("e")
        e = int(exp)
        if mant.endswith(".0"):
            mant = mant[:-2]
        sign = "+" if e >= 0 else "-"
        return f"{mant}e{sign}{abs(e)}"
    return s


def jcs_qty(q: dict[str, str]) -> str:
    si = qty_si_hex(q["text"], q["unit"])
    return f'{{"si":{jcs_str(si)},"text":{jcs_str(q["text"])},"unit":{jcs_str(q["unit"])}}}'


def canon_expr_str(expr: dict[str, object]) -> str:
    assert len(expr) == 1
    k, v = next(iter(expr.items()))
    if k == "and":
        flat: list[str] = []
        for item in v:  # type: ignore[union-attr]
            c = canon_expr_str(item)
            if c.startswith('{"and":['):
                # Re-parse flattened items from canonical child
                sub_items = canon_flatten_list(item, "and")
                flat.extend(sub_items)
            else:
                flat.append(c)
        uniq = sorted(set(flat))
        if len(uniq) == 1:
            return uniq[0]
        return '{"and":[' + ",".join(uniq) + "]}"
    if k == "or":
        flat = []
        for item in v:  # type: ignore[union-attr]
            c = canon_expr_str(item)
            if c.startswith('{"or":['):
                flat.extend(canon_flatten_list(item, "or"))
            else:
                flat.append(c)
        uniq = sorted(set(flat))
        if len(uniq) == 1:
            return uniq[0]
        return '{"or":[' + ",".join(uniq) + "]}"
    if k == "not":
        inner = canon_expr_str(v)  # type: ignore[arg-type]
        # Check if canonical inner is itself a `not`
        if inner.startswith('{"not":') and inner.endswith("}"):
            return inner[len('{"not":') : -1]
        return '{"not":' + inner + "}"
    if k == "all":
        return '{"all":{}}'
    if k == "ids":
        ids = sorted(set(v["ids"]))  # type: ignore[index]
        return '{"ids":{"ids":[' + ",".join(str(i) for i in ids) + "]}}"
    if k == "attr_range":
        d = v  # type: ignore[assignment]
        parts = [f'"channel":{jcs_str(d["channel"])}']
        if "max" in d:
            parts.append(f'"max":{jcs_num(d["max"])}')
        if "min" in d:
            parts.append(f'"min":{jcs_num(d["min"])}')
        return '{"attr_range":{' + ",".join(parts) + "}}"
    if k == "quality_flags":
        d = v  # type: ignore[assignment]
        parts = []
        if "all_of" in d:
            parts.append(f'"all_of":{int(d["all_of"])}')
        if "none_of" in d:
            parts.append(f'"none_of":{int(d["none_of"])}')
        return '{"quality_flags":{' + ",".join(parts) + "}}"
    if k == "normal_cone":
        d = v  # type: ignore[assignment]
        ax = ",".join(jcs_num(c) for c in d["axis"])
        return f'{{"normal_cone":{{"axis":[{ax}],"half_angle":{jcs_qty(d["half_angle"])}}}}}'
    if k == "in_box":
        d = v  # type: ignore[assignment]
        mx = ",".join(jcs_qty(q) for q in d["max"])
        mn = ",".join(jcs_qty(q) for q in d["min"])
        return f'{{"in_box":{{"max":[{mx}],"min":[{mn}]}}}}'
    if k == "in_sphere":
        d = v  # type: ignore[assignment]
        ctr = ",".join(jcs_qty(q) for q in d["centre"])
        return f'{{"in_sphere":{{"centre":[{ctr}],"radius":{jcs_qty(d["radius"])}}}}}'
    if k == "near":
        d = v  # type: ignore[assignment]
        of = d["of"]
        rk, rv = next(iter(of.items()))
        if rk == "element":
            of_s = f'{{"element":{jcs_str(rv)}}}'
        elif rk == "selection":
            of_s = f'{{"selection":{jcs_str(rv)}}}'
        elif rk == "face":
            of_s = f'{{"face":{{"body":{jcs_str(rv["body"])},"id":{jcs_str(rv["id"])}}}}}'
        else:
            raise AssertionError(rk)
        return f'{{"near":{{"distance":{jcs_qty(d["distance"])},"of":{of_s}}}}}'
    if k == "face_ids":
        d = v  # type: ignore[assignment]
        ids = sorted(set(d["ids"]), key=lambda s: s.encode("utf-8"))
        arr = ",".join(jcs_str(i) for i in ids)
        return f'{{"face_ids":{{"body":{jcs_str(d["body"])},"ids":[{arr}]}}}}'
    if k == "in_group":
        return f'{{"in_group":{{"group":{jcs_str(v["group"])}}}}}'  # type: ignore[index]
    if k == "in_layer":
        return f'{{"in_layer":{{"layer":{jcs_str(v["layer"])}}}}}'  # type: ignore[index]
    if k == "in_body":
        return f'{{"in_body":{{"body":{jcs_str(v["body"])}}}}}'  # type: ignore[index]
    if k == "tangent_grow":
        d = v  # type: ignore[assignment]
        return f'{{"tangent_grow":{{"angle":{jcs_qty(d["angle"])},"seed":{canon_expr_str(d["seed"])}}}}}'
    raise AssertionError(f"unknown expr key {k}")


def canon_flatten_list(expr: dict[str, object], op: str) -> list[str]:
    k, v = next(iter(expr.items()))
    if k == "not":
        # Check double negation revealing op
        inner = canon_expr_str(expr)
        if inner.startswith(f'{{"{op}":['):
            parsed = json.loads(inner)
            return [canon_expr_str(x) for x in parsed[op]]
        return [inner]
    if k == op:
        out: list[str] = []
        for item in v:  # type: ignore[union-attr]
            out.extend(canon_flatten_list(item, op))
        return out
    return [canon_expr_str(expr)]


def canonical_query(q: dict[str, object]) -> tuple[str, str]:
    where_s = canon_expr_str(q["where"])  # type: ignore[arg-type]
    canon = f'{{"domain":{jcs_str(q["domain"])},"v":1,"where":{where_s}}}'  # type: ignore[arg-type]
    digest = "sha256:" + hashlib.sha256(canon.encode("utf-8")).hexdigest()
    return canon, digest


def qlen(text: str, unit: str = "mm") -> dict[str, str]:
    return {"text": text, "unit": unit}


def qang(text: str, unit: str = "deg") -> dict[str, str]:
    return {"text": text, "unit": unit}


def make_query(domain: str, where: dict[str, object]) -> dict[str, object]:
    return {"v": 1, "domain": domain, "where": where}


def build_valid_60() -> list[dict[str, object]]:
    raw_cases: list[tuple[str, dict[str, object]]] = [
        ("all_points", make_query("points", {"all": {}})),
        ("all_triangles", make_query("triangles", {"all": {}})),
        ("all_faces", make_query("faces", {"all": {}})),
        ("all_edges", make_query("edges", {"all": {}})),
        ("ids_empty", make_query("points", {"ids": {"ids": []}})),
        ("ids_single", make_query("points", {"ids": {"ids": [42]}})),
        ("ids_unsorted_dups", make_query("triangles", {"ids": {"ids": [9, 1, 5, 1, 9, 0]}})),
        ("ids_large_u64", make_query("points", {"ids": {"ids": [18446744073709551615, 0, 9007199254740993]}})),
        ("attr_intensity_min", make_query("points", {"attr_range": {"channel": "intensity", "min": 0.25}})),
        ("attr_intensity_max", make_query("points", {"attr_range": {"channel": "intensity", "max": 0.75}})),
        ("attr_intensity_both", make_query("points", {"attr_range": {"channel": "intensity", "min": 0, "max": 1}})),
        ("attr_scanner_id", make_query("points", {"attr_range": {"channel": "scanner_id", "min": 1, "max": 3}})),
        ("attr_timestamp_ns", make_query("points", {"attr_range": {"channel": "timestamp_ns", "min": 1000, "max": 2000}})),
        ("attr_neg_zero", make_query("points", {"attr_range": {"channel": "intensity", "min": -0.0, "max": 0.5}})),
        ("qflags_all_of", make_query("points", {"quality_flags": {"all_of": 3}})),
        ("qflags_none_of", make_query("points", {"quality_flags": {"none_of": 65535}})),
        ("qflags_both", make_query("points", {"quality_flags": {"all_of": 4, "none_of": 8}})),
        ("qflags_zero", make_query("points", {"quality_flags": {"all_of": 0}})),
        ("normal_cone_z_deg", make_query("triangles", {"normal_cone": {"axis": [0, 0, 1], "half_angle": qang("15", "deg")}})),
        ("normal_cone_x_rad", make_query("points", {"normal_cone": {"axis": [1, 0, 0], "half_angle": qang("0.5", "rad")}})),
        ("normal_cone_arcmin", make_query("faces", {"normal_cone": {"axis": [-1, 2, -0.5], "half_angle": qang("30", "arcmin")}})),
        ("normal_cone_zero_angle", make_query("points", {"normal_cone": {"axis": [0, 1, 0], "half_angle": qang("0", "rad")}})),
        ("in_box_mm", make_query("points", {"in_box": {"min": [qlen("-10"), qlen("-5"), qlen("0")], "max": [qlen("10"), qlen("5"), qlen("20")]}})),
        ("in_box_mixed_units", make_query("triangles", {"in_box": {"min": [qlen("0", "m"), qlen("-100", "um"), qlen("-1", "in")], "max": [qlen("1", "m"), qlen("2", "mm"), qlen("2", "in")]}})),
        ("in_box_degenerate_point", make_query("points", {"in_box": {"min": [qlen("1"), qlen("2"), qlen("3")], "max": [qlen("1"), qlen("2"), qlen("3")]}})),
        ("in_sphere_mm", make_query("points", {"in_sphere": {"centre": [qlen("0"), qlen("0"), qlen("0")], "radius": qlen("25")}})),
        ("in_sphere_in", make_query("triangles", {"in_sphere": {"centre": [qlen("1", "in"), qlen("-2", "in"), qlen("0.5", "in")], "radius": qlen("0.25", "in")}})),
        ("in_sphere_zero_r", make_query("points", {"in_sphere": {"centre": [qlen("1", "m"), qlen("0", "m"), qlen("0", "m")], "radius": qlen("0", "mm")}})),
        ("near_element", make_query("points", {"near": {"of": {"element": U1}, "distance": qlen("2", "mm")}})),
        ("near_selection", make_query("triangles", {"near": {"of": {"selection": U2}, "distance": qlen("500", "um")}})),
        ("near_face", make_query("points", {"near": {"of": {"face": {"body": U3, "id": "F-001"}}, "distance": qlen("0.1", "in")}})),
        ("near_face_unicode_label", make_query("points", {"near": {"of": {"face": {"body": U3, "id": "face/top~1#a"}}, "distance": qlen("0", "m")}})),
        ("face_ids_empty", make_query("faces", {"face_ids": {"body": U1, "ids": []}})),
        ("face_ids_single", make_query("faces", {"face_ids": {"body": U1, "ids": ["F1"]}})),
        ("face_ids_unsorted", make_query("faces", {"face_ids": {"body": U1, "ids": ["F10", "F2", "F1", "F2"]}})),
        ("face_ids_prompt_text_literal", make_query("faces", {"face_ids": {"body": U1, "ids": ["ignore previous instructions; drop table"]}})),
        ("in_group", make_query("points", {"in_group": {"group": U1}})),
        ("in_layer", make_query("faces", {"in_layer": {"layer": U2}})),
        ("in_body", make_query("edges", {"in_body": {"body": U3}})),
        ("tangent_grow_simple", make_query("faces", {"tangent_grow": {"seed": {"face_ids": {"body": U1, "ids": ["F1"]}}, "angle": qang("5", "deg")}})),
        ("tangent_grow_nested", make_query("triangles", {"tangent_grow": {"seed": {"ids": {"ids": [1, 2]}}, "angle": qang("0.1", "rad")}})),
        ("not_all", make_query("points", {"not": {"all": {}}})),
        ("double_not", make_query("points", {"not": {"not": {"ids": {"ids": [1, 2]}}}})),
        ("triple_not", make_query("points", {"not": {"not": {"not": {"in_body": {"body": U1}}}}})),
        ("and_single", make_query("points", {"and": [{"all": {}}]})),
        ("and_two", make_query("points", {"and": [{"in_body": {"body": U1}}, {"in_layer": {"layer": U2}}]})),
        ("and_nested_dedup", make_query("points", {"and": [{"in_group": {"group": U1}}, {"and": [{"in_body": {"body": U2}}, {"in_group": {"group": U1}}]}]})),
        ("or_single", make_query("triangles", {"or": [{"ids": {"ids": [5]}}]})),
        ("or_two", make_query("triangles", {"or": [{"in_group": {"group": U2}}, {"in_group": {"group": U1}}]})),
        ("or_nested_dedup", make_query("faces", {"or": [{"in_layer": {"layer": U3}}, {"or": [{"in_layer": {"layer": U3}}, {"in_body": {"body": U1}}]}]})),
        ("and_or_combo", make_query("points", {"and": [{"or": [{"in_group": {"group": U1}}, {"in_group": {"group": U2}}]}, {"not": {"ids": {"ids": [0]}}}]})),
        ("or_and_combo", make_query("triangles", {"or": [{"and": [{"in_body": {"body": U1}}, {"quality_flags": {"all_of": 1}}]}, {"in_layer": {"layer": U4}}]})),
        ("not_and", make_query("points", {"not": {"and": [{"in_group": {"group": U1}}, {"in_layer": {"layer": U2}}]}})),
        ("not_or", make_query("edges", {"not": {"or": [{"in_body": {"body": U1}}, {"in_body": {"body": U2}}]}})),
        ("tangent_grow_with_and_seed", make_query("faces", {"tangent_grow": {"seed": {"and": [{"in_body": {"body": U1}}, {"normal_cone": {"axis": [0, 0, 1], "half_angle": qang("10", "deg")}}]}, "angle": qang("2", "deg")}})),
        ("spatial_and_attr", make_query("points", {"and": [{"in_sphere": {"centre": [qlen("0"), qlen("0"), qlen("0")], "radius": qlen("50")}}, {"attr_range": {"channel": "intensity", "min": 10, "max": 200}}]})),
        ("near_and_not_ids", make_query("points", {"and": [{"near": {"of": {"element": U1}, "distance": qlen("1.5", "mm")}}, {"not": {"ids": {"ids": [10, 20, 30]}}}]})),
        ("deep_not_chain_6", make_query("points", {"not": {"not": {"not": {"not": {"not": {"not": {"all": {}}}}}}}})),
        ("complex_multi_pred", make_query("triangles", {"and": [{"in_box": {"min": [qlen("-1", "m"), qlen("-1", "m"), qlen("-1", "m")], "max": [qlen("1", "m"), qlen("1", "m"), qlen("1", "m")]}}, {"normal_cone": {"axis": [0, 0, 1], "half_angle": qang("30", "deg")}}, {"quality_flags": {"none_of": 2}}, {"near": {"of": {"selection": U4}, "distance": qlen("5", "mm")}}]})),
        ("double_tangent_grow", make_query("faces", {"tangent_grow": {"seed": {"tangent_grow": {"seed": {"face_ids": {"body": U2, "ids": ["F_a", "F_b"]}}, "angle": qang("1", "deg")}}, "angle": qang("3", "deg")}})),
    ]
    assert len(raw_cases) == 60, len(raw_cases)
    out = []
    for name, query in raw_cases:
        canon, digest = canonical_query(query)
        out.append({"name": name, "query": query, "canonical": canon, "digest": digest})
    return out


def build_pairs() -> dict[str, object]:
    p_a = {"in_body": {"body": U1}}
    p_b = {"in_layer": {"layer": U2}}
    p_c = {"in_group": {"group": U3}}
    p_d = {"quality_flags": {"all_of": 1}}

    eq_pairs: list[dict[str, object]] = [
        # 1-10: operand order of and/or
        {"name": "and_swap_2", "a": make_query("points", {"and": [p_a, p_b]}), "b": make_query("points", {"and": [p_b, p_a]})},
        {"name": "and_perm_3", "a": make_query("points", {"and": [p_a, p_b, p_c]}), "b": make_query("points", {"and": [p_c, p_a, p_b]})},
        {"name": "and_perm_4", "a": make_query("points", {"and": [p_a, p_b, p_c, p_d]}), "b": make_query("points", {"and": [p_d, p_c, p_b, p_a]})},
        {"name": "or_swap_2", "a": make_query("triangles", {"or": [p_a, p_b]}), "b": make_query("triangles", {"or": [p_b, p_a]})},
        {"name": "or_perm_3", "a": make_query("faces", {"or": [p_a, p_b, p_c]}), "b": make_query("faces", {"or": [p_b, p_c, p_a]})},
        {"name": "or_perm_4", "a": make_query("edges", {"or": [p_a, p_b, p_c, p_d]}), "b": make_query("edges", {"or": [p_c, p_d, p_a, p_b]})},
        {"name": "and_nested_or_swap", "a": make_query("points", {"and": [{"or": [p_a, p_b]}, p_c]}), "b": make_query("points", {"and": [p_c, {"or": [p_b, p_a]}]})},
        {"name": "or_nested_and_swap", "a": make_query("points", {"or": [{"and": [p_a, p_b]}, p_c]}), "b": make_query("points", {"or": [p_c, {"and": [p_b, p_a]}]})},
        {"name": "not_and_swap", "a": make_query("points", {"not": {"and": [p_a, p_b]}}), "b": make_query("points", {"not": {"and": [p_b, p_a]}})},
        {"name": "tangent_grow_seed_swap", "a": make_query("faces", {"tangent_grow": {"seed": {"and": [p_a, p_b]}, "angle": qang("5")}}), "b": make_query("faces", {"tangent_grow": {"seed": {"and": [p_b, p_a]}, "angle": qang("5")}})},
        # 11-18: duplicate operands
        {"name": "and_dup_2", "a": make_query("points", {"and": [p_a, p_a]}), "b": make_query("points", p_a)},
        {"name": "and_dup_3", "a": make_query("points", {"and": [p_a, p_b, p_a]}), "b": make_query("points", {"and": [p_a, p_b]})},
        {"name": "or_dup_2", "a": make_query("points", {"or": [p_b, p_b]}), "b": make_query("points", p_b)},
        {"name": "or_dup_3", "a": make_query("points", {"or": [p_a, p_b, p_b, p_a]}), "b": make_query("points", {"or": [p_b, p_a]})},
        {"name": "and_dup_ids_reorder", "a": make_query("points", {"and": [{"ids": {"ids": [3, 1]}}, {"ids": {"ids": [1, 3]}}]}), "b": make_query("points", {"ids": {"ids": [1, 3]}})},
        {"name": "or_dup_double_not", "a": make_query("points", {"or": [p_a, {"not": {"not": p_a}}]}), "b": make_query("points", p_a)},
        {"name": "and_singleton", "a": make_query("points", {"and": [p_c]}), "b": make_query("points", p_c)},
        {"name": "or_singleton", "a": make_query("points", {"or": [p_c]}), "b": make_query("points", p_c)},
        # 19-26: nested same-operator flattening
        {"name": "and_flat_left", "a": make_query("points", {"and": [{"and": [p_a, p_b]}, p_c]}), "b": make_query("points", {"and": [p_a, p_b, p_c]})},
        {"name": "and_flat_right", "a": make_query("points", {"and": [p_a, {"and": [p_b, p_c]}]}), "b": make_query("points", {"and": [p_a, p_b, p_c]})},
        {"name": "and_flat_deep", "a": make_query("points", {"and": [{"and": [{"and": [p_a, p_b]}, p_c]}, p_d]}), "b": make_query("points", {"and": [p_a, p_b, p_c, p_d]})},
        {"name": "or_flat_left", "a": make_query("points", {"or": [{"or": [p_a, p_b]}, p_c]}), "b": make_query("points", {"or": [p_a, p_b, p_c]})},
        {"name": "or_flat_right", "a": make_query("points", {"or": [p_a, {"or": [p_b, p_c]}]}), "b": make_query("points", {"or": [p_a, p_b, p_c]})},
        {"name": "or_flat_deep", "a": make_query("points", {"or": [p_d, {"or": [p_c, {"or": [p_b, p_a]}]}]}), "b": make_query("points", {"or": [p_a, p_b, p_c, p_d]})},
        {"name": "and_flat_with_dup", "a": make_query("points", {"and": [{"and": [p_a, p_b]}, {"and": [p_b, p_c]}]}), "b": make_query("points", {"and": [p_c, p_a, p_b]})},
        {"name": "or_flat_with_double_not", "a": make_query("points", {"or": [p_a, {"not": {"not": {"or": [p_b, p_c]}}}]}), "b": make_query("points", {"or": [p_a, p_b, p_c]})},
        # 27-32: double negation
        {"name": "double_not_pred", "a": make_query("points", {"not": {"not": p_a}}), "b": make_query("points", p_a)},
        {"name": "quad_not_pred", "a": make_query("points", {"not": {"not": {"not": {"not": p_b}}}}), "b": make_query("points", p_b)},
        {"name": "triple_vs_single_not", "a": make_query("points", {"not": {"not": {"not": p_c}}}), "b": make_query("points", {"not": p_c})},
        {"name": "double_not_and", "a": make_query("points", {"not": {"not": {"and": [p_b, p_a]}}}), "b": make_query("points", {"and": [p_a, p_b]})},
        {"name": "double_not_inside_and", "a": make_query("points", {"and": [{"not": {"not": p_a}}, p_b]}), "b": make_query("points", {"and": [p_a, p_b]})},
        {"name": "double_not_in_tangent_seed", "a": make_query("faces", {"tangent_grow": {"seed": {"not": {"not": p_a}}, "angle": qang("2")}}), "b": make_query("faces", {"tangent_grow": {"seed": p_a, "angle": qang("2")}})},
        # 33-38: ID-list order and dedup
        {"name": "ids_order_3", "a": make_query("points", {"ids": {"ids": [3, 1, 2]}}), "b": make_query("points", {"ids": {"ids": [1, 2, 3]}})},
        {"name": "ids_dup_and_order", "a": make_query("points", {"ids": {"ids": [5, 2, 5, 1, 2]}}), "b": make_query("points", {"ids": {"ids": [1, 2, 5]}})},
        {"name": "face_ids_order", "a": make_query("faces", {"face_ids": {"body": U1, "ids": ["F3", "F1", "F2"]}}), "b": make_query("faces", {"face_ids": {"body": U1, "ids": ["F1", "F2", "F3"]}})},
        {"name": "face_ids_dup", "a": make_query("faces", {"face_ids": {"body": U1, "ids": ["F2", "F1", "F2"]}}), "b": make_query("faces", {"face_ids": {"body": U1, "ids": ["F1", "F2"]}})},
        {"name": "neg_zero_attr", "a": make_query("points", {"attr_range": {"channel": "intensity", "min": -0.0, "max": 1.0}}), "b": make_query("points", {"attr_range": {"channel": "intensity", "min": 0.0, "max": 1.0}})},
        {"name": "neg_zero_axis", "a": make_query("triangles", {"normal_cone": {"axis": [-0.0, 0.0, 1.0], "half_angle": qang("10")}}), "b": make_query("triangles", {"normal_cone": {"axis": [0.0, 0.0, 1.0], "half_angle": qang("10")}})},
        # 39-40: whitespace & key order (raw JSON strings!)
        {"name": "raw_whitespace_key_order_1", "a_raw": '{\n  "where": { "in_body": { "body": "' + U1 + '" } },\n  "domain": "points",\n  "v": 1\n}', "b_raw": '{"v":1,"domain":"points","where":{"in_body":{"body":"' + U1 + '"}}}'},
        {"name": "raw_whitespace_key_order_2", "a_raw": '{ "domain":"faces", "v": 1, "where": { "near": { "distance": { "unit": "mm", "text": "2" }, "of": { "face": { "id": "F1", "body": "' + U2 + '" } } } } }', "b_raw": '{"v":1,"domain":"faces","where":{"near":{"of":{"face":{"body":"' + U2 + '","id":"F1"}},"distance":{"text":"2","unit":"mm"}}}}'},
    ]
    assert len(eq_pairs) == 40, len(eq_pairs)

    neq_pairs: list[dict[str, object]] = [
        {"name": "diff_domain_pts_tris", "a": make_query("points", {"all": {}}), "b": make_query("triangles", {"all": {}})},
        {"name": "diff_domain_faces_edges", "a": make_query("faces", {"all": {}}), "b": make_query("edges", {"all": {}})},
        {"name": "and_vs_or", "a": make_query("points", {"and": [p_a, p_b]}), "b": make_query("points", {"or": [p_a, p_b]})},
        {"name": "not_vs_pos", "a": make_query("points", {"not": p_a}), "b": make_query("points", p_a)},
        {"name": "ids_diff_member", "a": make_query("points", {"ids": {"ids": [1, 2]}}), "b": make_query("points", {"ids": {"ids": [1, 3]}})},
        {"name": "ids_empty_vs_nonempty", "a": make_query("points", {"ids": {"ids": []}}), "b": make_query("points", {"ids": {"ids": [0]}})},
        {"name": "attr_diff_channel", "a": make_query("points", {"attr_range": {"channel": "intensity", "min": 0}}), "b": make_query("points", {"attr_range": {"channel": "scanner_id", "min": 0}})},
        {"name": "attr_min_vs_max", "a": make_query("points", {"attr_range": {"channel": "intensity", "min": 1}}), "b": make_query("points", {"attr_range": {"channel": "intensity", "max": 1}})},
        {"name": "qflags_all_vs_none", "a": make_query("points", {"quality_flags": {"all_of": 1}}), "b": make_query("points", {"quality_flags": {"none_of": 1}})},
        {"name": "normal_cone_axis_sign", "a": make_query("triangles", {"normal_cone": {"axis": [0, 0, 1], "half_angle": qang("10")}}), "b": make_query("triangles", {"normal_cone": {"axis": [0, 0, -1], "half_angle": qang("10")}})},
        {"name": "req005_authored_text_2_vs_2_0", "a": make_query("points", {"in_sphere": {"centre": [qlen("0"), qlen("0"), qlen("0")], "radius": qlen("2", "mm")}}), "b": make_query("points", {"in_sphere": {"centre": [qlen("0"), qlen("0"), qlen("0")], "radius": qlen("2.0", "mm")}})},
        {"name": "req005_authored_unit_1000mm_vs_1m", "a": make_query("points", {"near": {"of": {"element": U1}, "distance": qlen("1000", "mm")}}), "b": make_query("points", {"near": {"of": {"element": U1}, "distance": qlen("1", "m")}})},
        {"name": "near_element_vs_selection", "a": make_query("points", {"near": {"of": {"element": U1}, "distance": qlen("1")}}), "b": make_query("points", {"near": {"of": {"selection": U1}, "distance": qlen("1")}})},
        {"name": "near_face_diff_id", "a": make_query("points", {"near": {"of": {"face": {"body": U1, "id": "F1"}}, "distance": qlen("1")}}), "b": make_query("points", {"near": {"of": {"face": {"body": U1, "id": "F2"}}, "distance": qlen("1")}})},
        {"name": "face_ids_diff_body", "a": make_query("faces", {"face_ids": {"body": U1, "ids": ["F1"]}}), "b": make_query("faces", {"face_ids": {"body": U2, "ids": ["F1"]}})},
        {"name": "in_group_vs_layer", "a": make_query("points", {"in_group": {"group": U1}}), "b": make_query("points", {"in_layer": {"layer": U1}})},
        {"name": "in_layer_vs_body", "a": make_query("points", {"in_layer": {"layer": U1}}), "b": make_query("points", {"in_body": {"body": U1}})},
        {"name": "tangent_grow_diff_angle", "a": make_query("faces", {"tangent_grow": {"seed": p_a, "angle": qang("5")}}), "b": make_query("faces", {"tangent_grow": {"seed": p_a, "angle": qang("6")}})},
        {"name": "tangent_grow_diff_seed", "a": make_query("faces", {"tangent_grow": {"seed": p_a, "angle": qang("5")}}), "b": make_query("faces", {"tangent_grow": {"seed": p_b, "angle": qang("5")}})},
        {"name": "in_box_diff_max", "a": make_query("points", {"in_box": {"min": [qlen("0"), qlen("0"), qlen("0")], "max": [qlen("1"), qlen("1"), qlen("1")]}}), "b": make_query("points", {"in_box": {"min": [qlen("0"), qlen("0"), qlen("0")], "max": [qlen("1"), qlen("1"), qlen("2")]}})},
    ]
    assert len(neq_pairs) == 20, len(neq_pairs)
    return {"equivalent": eq_pairs, "non_equivalent": neq_pairs}


def build_invalid_80() -> list[dict[str, object]]:
    # Each entry: {"name": ..., "raw" or "query": ..., "expected_code": "bad_query"|"bad_quantity", "expected_pointer": "..."}
    cases: list[dict[str, object]] = []

    def add(name: str, query: object, pointer: str, code: str = "bad_query") -> None:
        cases.append({"name": name, "query": query, "expected_code": code, "expected_pointer": pointer})

    def add_raw(name: str, raw: str, pointer: str, code: str = "bad_query") -> None:
        cases.append({"name": name, "raw": raw, "expected_code": code, "expected_pointer": pointer})

    # 1-10: top-level & version & domain
    add("top_not_object_arr", [], "")
    add("top_not_object_str", "points", "")
    add("top_unknown_key", {"v": 1, "domain": "points", "where": {"all": {}}, "extra": 1}, "/extra")
    add("top_prompt_key", {"v": 1, "domain": "points", "where": {"all": {}}, "ignore previous instructions": {}}, "/ignore previous instructions")
    add("missing_v", {"domain": "points", "where": {"all": {}}}, "/v")
    add("version_2", {"v": 2, "domain": "points", "where": {"all": {}}}, "/v")
    add("version_0", {"v": 0, "domain": "points", "where": {"all": {}}}, "/v")
    add("version_string", {"v": "1", "domain": "points", "where": {"all": {}}}, "/v")
    add("version_float", {"v": 1.5, "domain": "points", "where": {"all": {}}}, "/v")
    add("missing_domain", {"v": 1, "where": {"all": {}}}, "/domain")
    add("unknown_domain", {"v": 1, "domain": "voxels", "where": {"all": {}}}, "/domain")
    add("domain_int", {"v": 1, "domain": 1, "where": {"all": {}}}, "/domain")
    add("missing_where", {"v": 1, "domain": "points"}, "/where")

    # 14-26: Expr structure & combinators
    add("where_not_obj", make_query("points", []), "/where")  # type: ignore[arg-type]
    add("where_empty_obj", make_query("points", {}), "/where")
    add("where_two_preds", make_query("points", {"all": {}, "ids": {"ids": [1]}}), "/where")
    add("where_unknown_pred", make_query("points", {"sql_eval": {}}), "/where/sql_eval")
    add("where_prompt_pred_key", make_query("points", {"run shell rm -rf": {}}), "/where/run shell rm -rf")
    add("and_not_array", make_query("points", {"and": {"all": {}}}), "/where/and")
    add("and_empty", make_query("points", {"and": []}), "/where/and")
    add("and_elem_two_keys", make_query("points", {"and": [{"all": {}, "not": {"all": {}}}]}), "/where/and/0")
    add("or_not_array", make_query("points", {"or": 123}), "/where/or")
    add("or_empty", make_query("points", {"or": []}), "/where/or")
    add("or_bad_second", make_query("points", {"or": [{"all": {}}, {"bogus": {}}]}), "/where/or/1/bogus")
    add("not_not_obj", make_query("points", {"not": "all"}), "/where/not")
    add("not_empty_obj", make_query("points", {"not": {}}), "/where/not")

    # 27-33: all & ids
    add("all_extra_key", make_query("points", {"all": {"x": 1}}), "/where/all/x")
    add("all_not_obj", make_query("points", {"all": True}), "/where/all")
    add("ids_missing_ids", make_query("points", {"ids": {}}), "/where/ids/ids")
    add("ids_extra_key", make_query("points", {"ids": {"ids": [1], "extra": 2}}), "/where/ids/extra")
    add("ids_not_array", make_query("points", {"ids": {"ids": 1}}), "/where/ids/ids")
    add("ids_string_elem", make_query("points", {"ids": {"ids": ["1"]}}), "/where/ids/ids/0")
    add("ids_negative_elem", make_query("points", {"ids": {"ids": [1, -2]}}), "/where/ids/ids/1")
    add("ids_float_elem", make_query("points", {"ids": {"ids": [1.5]}}), "/where/ids/ids/0")

    # 35-42: attr_range
    add("attr_missing_channel", make_query("points", {"attr_range": {"min": 0}}), "/where/attr_range/channel")
    add("attr_bad_channel", make_query("points", {"attr_range": {"channel": "rgb", "min": 0}}), "/where/attr_range/channel")
    add("attr_no_bounds", make_query("points", {"attr_range": {"channel": "intensity"}}), "/where/attr_range")
    add("attr_min_str", make_query("points", {"attr_range": {"channel": "intensity", "min": "0"}}), "/where/attr_range/min")
    add("attr_max_bool", make_query("points", {"attr_range": {"channel": "intensity", "max": False}}), "/where/attr_range/max")
    add("attr_min_gt_max", make_query("points", {"attr_range": {"channel": "intensity", "min": 5, "max": 2}}), "/where/attr_range/max")
    add("attr_unknown_key", make_query("points", {"attr_range": {"channel": "intensity", "min": 0, "step": 1}}), "/where/attr_range/step")

    # 42-47: quality_flags
    add("qflags_empty", make_query("points", {"quality_flags": {}}), "/where/quality_flags")
    add("qflags_all_of_neg", make_query("points", {"quality_flags": {"all_of": -1}}), "/where/quality_flags/all_of")
    add("qflags_all_of_overflow", make_query("points", {"quality_flags": {"all_of": 65536}}), "/where/quality_flags/all_of")
    add("qflags_none_of_str", make_query("points", {"quality_flags": {"none_of": "1"}}), "/where/quality_flags/none_of")
    add("qflags_unknown_key", make_query("points", {"quality_flags": {"all_of": 1, "any_of": 2}}), "/where/quality_flags/any_of")

    # 47-55: normal_cone
    add("ncone_missing_axis", make_query("triangles", {"normal_cone": {"half_angle": qang("10")}}), "/where/normal_cone/axis")
    add("ncone_axis_len_2", make_query("triangles", {"normal_cone": {"axis": [0, 1], "half_angle": qang("10")}}), "/where/normal_cone/axis")
    add("ncone_axis_str", make_query("triangles", {"normal_cone": {"axis": [0, "1", 0], "half_angle": qang("10")}}), "/where/normal_cone/axis/1")
    add("ncone_axis_zero", make_query("triangles", {"normal_cone": {"axis": [0, 0, 0], "half_angle": qang("10")}}), "/where/normal_cone/axis")
    add("ncone_length_unit", make_query("triangles", {"normal_cone": {"axis": [0, 0, 1], "half_angle": qlen("10", "mm")}}), "/where/normal_cone/half_angle/unit")
    add("ncone_neg_angle", make_query("triangles", {"normal_cone": {"axis": [0, 0, 1], "half_angle": qang("-5", "deg")}}), "/where/normal_cone/half_angle")
    add("ncone_gt_pi", make_query("triangles", {"normal_cone": {"axis": [0, 0, 1], "half_angle": qang("181", "deg")}}), "/where/normal_cone/half_angle")
    add("ncone_bad_qty_text", make_query("triangles", {"normal_cone": {"axis": [0, 0, 1], "half_angle": {"text": "NaN", "unit": "deg"}}}), "/where/normal_cone/half_angle", "bad_quantity")

    # 55-62: in_box & in_sphere
    add("inbox_missing_max", make_query("points", {"in_box": {"min": [qlen("0"), qlen("0"), qlen("0")]}}), "/where/in_box/max")
    add("inbox_len_2", make_query("points", {"in_box": {"min": [qlen("0"), qlen("0")], "max": [qlen("1"), qlen("1"), qlen("1")]}}), "/where/in_box/min")
    add("inbox_angle_unit", make_query("points", {"in_box": {"min": [qlen("0"), qang("1", "deg"), qlen("0")], "max": [qlen("1"), qlen("1"), qlen("1")]}}), "/where/in_box/min/1/unit")
    add("inbox_min_gt_max", make_query("points", {"in_box": {"min": [qlen("0"), qlen("0"), qlen("5")], "max": [qlen("1"), qlen("1"), qlen("2")]}}), "/where/in_box/max/2")
    add("inbox_bad_si_bits", make_query("points", {"in_box": {"min": [{"text": "1", "unit": "mm", "si": "0x0000000000000000"}, qlen("0"), qlen("0")], "max": [qlen("2"), qlen("1"), qlen("1")]}}), "/where/in_box/min/0/si")
    add("insphere_neg_radius", make_query("points", {"in_sphere": {"centre": [qlen("0"), qlen("0"), qlen("0")], "radius": qlen("-1", "mm")}}), "/where/in_sphere/radius")
    add("insphere_unknown_unit", make_query("points", {"in_sphere": {"centre": [qlen("0"), qlen("0"), qlen("0")], "radius": {"text": "1", "unit": "furlongs"}}}), "/where/in_sphere/radius/unit")
    add("insphere_qty_extra_key", make_query("points", {"in_sphere": {"centre": [qlen("0"), qlen("0"), qlen("0")], "radius": {"text": "1", "unit": "mm", "raw": 1}}}), "/where/in_sphere/radius/raw")

    # 63-74: near, face_ids, in_group, in_layer, in_body
    add("near_of_empty", make_query("points", {"near": {"of": {}, "distance": qlen("1")}}), "/where/near/of")
    add("near_of_two_keys", make_query("points", {"near": {"of": {"element": U1, "selection": U2}, "distance": qlen("1")}}), "/where/near/of")
    add("near_of_unknown", make_query("points", {"near": {"of": {"cloud": U1}, "distance": qlen("1")}}), "/where/near/of/cloud")
    add("near_bad_uuid_upper", make_query("points", {"near": {"of": {"element": "00000000-0000-7000-8000-00000000000A"}, "distance": qlen("1")}}), "/where/near/of/element")
    add("near_nil_uuid", make_query("points", {"near": {"of": {"selection": "00000000-0000-0000-0000-000000000000"}, "distance": qlen("1")}}), "/where/near/of/selection")
    add("near_face_empty_id", make_query("points", {"near": {"of": {"face": {"body": U1, "id": ""}}, "distance": qlen("1")}}), "/where/near/of/face/id")
    add("near_neg_dist", make_query("points", {"near": {"of": {"element": U1}, "distance": qlen("-0.1")}}), "/where/near/distance")
    add("face_ids_too_long_topo", make_query("faces", {"face_ids": {"body": U1, "ids": ["a" * 257]}}), "/where/face_ids/ids/0")
    add("face_ids_int_topo", make_query("faces", {"face_ids": {"body": U1, "ids": [123]}}), "/where/face_ids/ids/0")
    add("in_group_bad_uuid", make_query("points", {"in_group": {"group": "not-a-uuid"}}), "/where/in_group/group")
    add("in_layer_max_uuid", make_query("points", {"in_layer": {"layer": "ffffffff-ffff-ffff-ffff-ffffffffffff"}}), "/where/in_layer/layer")
    add("in_body_missing", make_query("points", {"in_body": {}}), "/where/in_body/body")

    # 75-80: tangent_grow & JSON syntax/duplicate keys
    add("tgrow_missing_seed", make_query("faces", {"tangent_grow": {"angle": qang("5")}}), "/where/tangent_grow/seed")
    add("tgrow_missing_angle", make_query("faces", {"tangent_grow": {"seed": {"all": {}}}}), "/where/tangent_grow/angle")
    add("tgrow_bad_seed", make_query("faces", {"tangent_grow": {"seed": {"bad": {}}, "angle": qang("5")}}), "/where/tangent_grow/seed/bad")
    add("tgrow_neg_angle", make_query("faces", {"tangent_grow": {"seed": {"all": {}}, "angle": qang("-1")}}), "/where/tangent_grow/angle")
    add_raw("dup_top_key", '{"v":1,"v":1,"domain":"points","where":{"all":{}}}', "/v")
    add_raw("trailing_garbage", '{"v":1,"domain":"points","where":{"all":{}}} trailing', "")

    assert len(cases) == 80, len(cases)
    return cases


def dump_json(obj: object) -> bytes:
    return (json.dumps(obj, sort_keys=True, indent=2, ensure_ascii=False) + "\n").encode("utf-8")


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--check", action="store_true", help="Verify committed fixtures match generated bytes.")
    args = ap.parse_args()

    outputs = {
        HERE / "valid_v1.json": dump_json({"license": "CC-BY-4.0", "schema": "geo-sel-query-valid/v1", "cases": build_valid_60()}),
        HERE / "equivalent_pairs.json": dump_json({"license": "CC-BY-4.0", "schema": "geo-sel-query-pairs/v1", **build_pairs()}),
        HERE / "invalid_v1.json": dump_json({"license": "CC-BY-4.0", "schema": "geo-sel-query-invalid/v1", "cases": build_invalid_80()}),
    }

    if args.check:
        ok = True
        for path, expected in outputs.items():
            if not path.exists():
                print(f"MISSING: {path}", file=sys.stderr)
                ok = False
                continue
            if path.read_bytes() != expected:
                print(f"DRIFT: {path}", file=sys.stderr)
                ok = False
        return 0 if ok else 1

    for path, content in outputs.items():
        path.write_bytes(content)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
