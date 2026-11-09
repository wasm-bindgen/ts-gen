import { assert } from "./harness.mjs";

class Probe {
  echo(v) {
    return v + "|";
  }
}
for (const n of ["Str", "OptStr", "Strs", "Pair", "Body"]) {
  Probe.prototype["make" + n] = function () {
    return this.next;
  };
  Probe.prototype["see" + n] = function (v) {
    this.seen = v;
  };
}
globalThis.Probe = Probe;

const fs = await import("node:fs");
const bytes = fs.readFileSync("bridge.wasm");
const lang = new WebAssembly.Module(fs.readFileSync("lang.wasm"));

// Once with the js-string builtins, once through the polyfill.
for (const builtins of [true, false]) {
  const { instantiate, scratchUse } = await import(`./bridge.js?builtins=${builtins}`);
  const bridge = new WebAssembly.Module(bytes, builtins ? { builtins: ["js-string"] } : {});
  const ex = instantiate(lang, bridge).exports;
  const p = new Probe();
  const base = scratchUse();
  const call = (k, ...a) => ex[Object.keys(ex).find((x) => x.startsWith(`call:Probe#${k}(`))](p, ...a);
  const roundTrip = (n, v, ...rest) => {
    const expected = rest.length ? rest[0] : v;
    p.next = v;
    const g = call("make" + n);
    call("see" + n, ...(Array.isArray(g) ? g : [g]));
    assert.deepEqual(p.seen, expected, `${n} (builtins: ${builtins})`);
    assert.deepEqual(scratchUse(), base);
  };

  // WTF-16 keeps lone surrogates.
  for (const s of ["", "abc", "😀", "a\ud800b", "\udc00", "x".repeat(50000)]) roundTrip("Str", s);
  roundTrip("OptStr", null, undefined);
  roundTrip("OptStr", "o");
  roundTrip("Strs", ["a", "\ud83d", ""]);
  roundTrip("Pair", [5, "p"]);
  roundTrip("Body", "t");
  roundTrip("Body", 2);
  p.next = 5;
  assert.throws(() => call("makeStr"), TypeError);
  p.next = ["a", 1];
  assert.throws(() => call("makeStrs"), TypeError);
  assert.deepEqual(scratchUse(), base);

  // A string param and result in one call.
  p.next = "in";
  const g = call("makeStr");
  const r = call("echo", g);
  call("seeStr", r);
  assert.equal(p.seen, "in|");
}

console.log("ok");
