// CORE.REG.MAN AC-3: recompute every definition_digest and the manifest digest of the
// golden manifests with the JavaScript engine's own JSON (the canonicaliser of
// fixtures/core/jrn/canon/runner.mjs, repeated here because importing that file runs its
// vector check). Also confirms that tampered.json fails the definition-digest check.
//
//   node fixtures/core/reg/manifest/check_digests.mjs
import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

function canonicalize(v) {
  if (v === null || typeof v === "boolean") return JSON.stringify(v);
  if (typeof v === "number") {
    if (!Number.isFinite(v)) throw new Error("non_finite_number");
    return JSON.stringify(v);
  }
  if (typeof v === "string") return JSON.stringify(v);
  if (Array.isArray(v)) return "[" + v.map(canonicalize).join(",") + "]";
  const keys = Object.keys(v).sort(); // UTF-16 code-unit order
  return "{" + keys.map((k) => JSON.stringify(k) + ":" + canonicalize(v[k])).join(",") + "}";
}

const sha = (s) => "sha256:" + createHash("sha256").update(Buffer.from(s, "utf8")).digest("hex");
const file = (name) => readFileSync(fileURLToPath(new URL("./" + name, import.meta.url)), "utf8");

// Returns the number of definitions whose stated digest does not match.
function mismatches(text) {
  let bad = 0;
  for (const c of JSON.parse(text).commands) {
    const { definition_digest, ...body } = c;
    if (sha(canonicalize(body)) !== definition_digest) bad++;
  }
  return bad;
}

let failed = 0;
for (const name of ["valid", "order"]) {
  const text = file(name + ".manifest.json");
  if (canonicalize(JSON.parse(text)) !== text) { failed++; console.error(`${name}: not canonical`); }
  if (mismatches(text) !== 0) { failed++; console.error(`${name}: definition_digest mismatch`); }
  if (sha(text) !== file(name + ".digest").trimEnd()) { failed++; console.error(`${name}: manifest digest mismatch`); }
}
if (mismatches(file("tampered.json")) !== 1) { failed++; console.error("tampered: expected exactly one mismatch"); }
console.log(failed ? `manifest digests: ${failed} failure(s)` : "manifest digests: ok");
process.exit(failed ? 1 : 0);
