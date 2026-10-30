// Runs wasm-bindgen over the integration cdylib and checks the JS namespace
// the `--provides` wrapper publishes: a top-level function, nested objects,
// and `export default { ... }`.
import { execFileSync } from "node:child_process";
import { mkdtempSync, readdirSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { pathToFileURL } from "node:url";
import assert from "node:assert/strict";

const [wasm, wasmBindgen] = process.argv.slice(2);
const out = mkdtempSync(join(tmpdir(), "ts-gen-provides-"));
execFileSync(wasmBindgen, ["--target", "experimental-nodejs-module", "--out-dir", out, wasm], {
  stdio: "inherit",
});
const js = readdirSync(out).find((f) => f.endsWith(".js") && !f.endsWith("_bg.js"));
globalThis.Ticket = class Ticket {
  constructor(id) {
    this.id = id;
  }
};
const m = await import(pathToFileURL(join(out, js)));

assert.equal(m.version(), 3);
assert.equal(m.api.math.add(2, 3), 5);
assert.equal(m.api.str.add("a", "b"), "ab");
// The integration crate is one cdylib, so `default` also carries the other
// fixtures' default-object members.
assert.equal(typeof m.default, "object");
for (const key of ["describe", "issue"]) assert.equal(typeof m.default[key], "function");
assert.equal(m.default.issue(7).id, 7);
assert.equal(m.default.describe(m.default.issue(7)), "ticket 7");
assert.equal(m.default.describe(null), "no ticket");
// Top-level `add` and `default.add` are distinct functions.
assert.equal(m.add(2, 3), 5);
assert.equal(m.default.add(2), 102);
// js_sys-typed providers.
assert.equal(m.sum({ a: 1, b: 2 }), 3);
assert.equal(m.describeSymbol(Symbol("tag")), "Symbol(tag)");
assert.equal(m.double(21n), 42n);
assert.deepEqual(m.keys({ a: 1 }), ["a"]);
let seen;
m.notify((e, n) => { seen = [e, n]; });
assert.deepEqual(seen, [null, 1]);
assert.equal(m.retOpt(null), "none");
assert.equal(m.retOpt("x"), "x");
// A `string | null` result returns `Nullable<String>`: JS receives `null`,
// not `undefined`.
assert.equal(m.find(7), "seven");
assert.strictEqual(m.find(1), null);
console.log("provides namespace ok");
