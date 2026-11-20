import { assert, lang } from "./harness.mjs";

class Pair {
  get first() {
    return "ignored";
  }
  get second() {
    return "s";
  }
}
class Probe {
  constructor() {
    this.seen = [];
    this.next = undefined;
  }
}
for (const m of ["take", "give", "pair", "boxed", "pairs"]) {
  Probe.prototype[m] = function (...args) {
    this.seen.push(args);
    return this.next;
  };
}
Object.assign(globalThis, { Pair, Probe });

const { instantiate } = await import("./bridge.js");
const fs = await import("node:fs");
const compile = (f) => new WebAssembly.Module(fs.readFileSync(f));
const L = lang(instantiate(compile("lang.wasm"), compile("bridge.wasm")));
const p = new Probe();
const call = (key, next, ...wire) => {
  p.next = next;
  return L.call(key, p, ...wire);
};
const seen = () => p.seen.pop();

// A `unit` value has no wire: JS sees `undefined`, and what JS gives back
// is ignored, not checked.
L.balanced(() => {
  const [ptr, len] = call("Probe#take(unit,str)", "r", ...L.str("x"));
  assert.equal(L.takeStr(ptr, len), "r");
});
assert.deepEqual(seen(), [undefined, "x"]);
assert.equal(call("Probe#give()", 5), undefined);
assert.equal(call("Probe#pair((unit,u32))", ["dropped", 9], 7), 9);
assert.deepEqual(seen(), [[undefined, 7]]);

// `Unit` boxes `unit`: JS sees `undefined`, and the language `ref.null`.
assert.equal(call("Probe#boxed(Unit)", undefined, null), null);
assert.deepEqual(seen(), [undefined]);

// A mono `unit` argument has no wire either.
const pair = new Pair();
assert.equal(L.call("Pair#get:first<unit,Str>()", pair), undefined);
assert.equal(L.call("Pair#get:second<unit,Str>()", pair), "s");

console.log("ok");
