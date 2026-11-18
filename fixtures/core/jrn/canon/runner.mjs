// Second, independent implementation of RFC 8785 canonical JSON (CORE.JRN.CANON REQ-008).
// Uses the JavaScript engine's own number formatting and UTF-16 string ordering,
// which RFC 8785 is defined against, so it is the reference for the Rust crate.
//
//   node fixtures/core/jrn/canon/runner.mjs            verify vectors.json (CI)
//   node fixtures/core/jrn/canon/runner.mjs generate   regenerate vectors.json
import { createHash } from "node:crypto";
import { readFileSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

const FILE = fileURLToPath(new URL("./vectors.json", import.meta.url));

export function canonicalize(v) {
  if (v === null || typeof v === "boolean") return JSON.stringify(v);
  if (typeof v === "number") {
    if (!Number.isFinite(v)) throw new Error("non_finite_number");
    return JSON.stringify(v); // ECMAScript Number::toString; -0 prints as 0
  }
  if (typeof v === "string") return JSON.stringify(v);
  if (Array.isArray(v)) return "[" + v.map(canonicalize).join(",") + "]";
  const keys = Object.keys(v).sort(); // default sort compares UTF-16 code units
  return "{" + keys.map((k) => JSON.stringify(k) + ":" + canonicalize(v[k])).join(",") + "}";
}

const digest = (s) => "sha256:" + createHash("sha256").update(Buffer.from(s, "utf8")).digest("hex");

// Deterministic PRNG (mulberry32) so regeneration is reproducible.
function rng(seed) {
  return () => {
    seed |= 0; seed = (seed + 0x6d2b79f5) | 0;
    let t = Math.imul(seed ^ (seed >>> 15), 1 | seed);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

function inputs() {
  const out = [];
  const add = (name, text) => out.push({ name, input: text });
  // Literals, strings and escapes
  for (const t of ["null", "true", "false", '""', '"a"', '"\\u0000\\u001f"', '"\\b\\f\\n\\r\\t"', '"\\"\\\\\\/"',
    '"\\u00e9\\u20ac"', '"\\ud83d\\ude00"', '"é€😀"', '"\\u007f"', '"\\u2028\\u2029"', '"line\\nbreak"'])
    add("scalar " + t, t);
  // Numbers: boundaries and ECMAScript formatting edges
  for (const t of ["0", "-0", "0.0", "1", "-1", "1.5", "0.1", "0.2", "0.30000000000000004", "1e21", "1e20", "123e18",
    "1e-6", "1e-7", "0.000001", "0.0000001", "1.2345e-7", "5e-324", "-5e-324", "1.7976931348623157e308",
    "2.2250738585072014e-308", "9007199254740992", "9007199254740993", "18446744073709551615",
    "-9223372036854775808", "4.5", "100", "1E2", "1e+2", "12345678901234567890", "0.1e1", "333333333.33333329",
    "1e23", "8.41e21", "5e-7", "1.0000000000000002", "123456789012345680000", "1e-5", "-1e-7"])
    add("number " + t, t);
  // Whitespace and structure
  add("ws array", ' [ 1 , 2 , [ ] , { } ] ');
  add("ws object", '\n{\t"b" : 1 ,\r\n "a" : [ true , null ] }\n');
  add("nested", '{"a":{"b":{"c":{"d":[1,[2,[3,[4]]]]}}}}');
  add("empty object", "{}");
  add("empty array", "[]");
  // Key ordering by UTF-16 code units (RFC 8785 §3.2.3), incl. astral vs BMP-private-use
  add("keys ascii", '{"b":1,"a":2,"aa":3,"A":4,"_":5,"1":6,"":7}');
  add("keys unicode", '{"\\u20ac":1,"\\r":2,"\\ufb33":3,"1":4,"\\ud83d\\ude00":5,"\\u0080":6,"\\u00f6":7,"\\ue000":8}');
  add("keys astral vs e000", '{"\\ud800\\udc00":1,"\\uffff":2,"\\ue000":3}');
  add("rfc8785 sample", '{"numbers":[333333333.33333329,1E30,4.50,2e-3,0.000000000000000000000000001],' +
    '"string":"\\u20ac$\\u000F\\u000aA\'\\u0042\\u0022\\u005c\\\\\\"\\/","literals":[null,true,false]}');
  // Generated doubles from random bit patterns, written in several notations
  const r = rng(8785);
  const buf = new DataView(new ArrayBuffer(8));
  let made = 0;
  while (made < 120) {
    buf.setUint32(0, Math.floor(r() * 2 ** 32));
    buf.setUint32(4, Math.floor(r() * 2 ** 32));
    const x = buf.getFloat64(0);
    if (!Number.isFinite(x)) continue;
    const notation = made % 3;
    const text = notation === 0 ? JSON.stringify(x) : notation === 1 ? x.toExponential(20) : x.toPrecision(17);
    add(`double ${made}`, text);
    made++;
  }
  // Generated decimals in a human range
  for (let i = 0; i < 40; i++) {
    const x = Math.round((r() - 0.5) * 2e6) / 10 ** Math.floor(r() * 9);
    add(`decimal ${i}`, String(x));
  }
  // Generated objects with mixed keys
  const alphabet = ["a", "B", "z", "é", "€", "😀", "\u{e000}", "_", "0", "aa"];
  for (let i = 0; i < 20; i++) {
    const o = {};
    for (let j = 0; j < 6; j++) o[alphabet[Math.floor(r() * alphabet.length)] + j] = Math.floor(r() * 1000) / 8;
    add(`object ${i}`, JSON.stringify(o, null, j2(i)));
  }
  return out;
}
const j2 = (i) => (i % 2 ? 2 : 0);

if (process.argv[2] === "generate") {
  const vectors = inputs().map((v) => {
    const canonical = canonicalize(JSON.parse(v.input));
    return { ...v, canonical, digest: digest(canonical) };
  });
  writeFileSync(FILE, JSON.stringify(vectors, null, 1) + "\n");
  console.log(`wrote ${vectors.length} vectors to ${FILE}`);
} else {
  const vectors = JSON.parse(readFileSync(FILE, "utf8"));
  let bad = 0;
  for (const v of vectors) {
    const c = canonicalize(JSON.parse(v.input));
    if (c !== v.canonical || digest(c) !== v.digest) {
      bad++;
      console.error(`MISMATCH ${v.name}: got ${c}`);
    }
  }
  console.log(`${vectors.length - bad}/${vectors.length} vectors reproduced`);
  process.exit(bad ? 1 : 0);
}
