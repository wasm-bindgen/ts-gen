import { assert, lang } from "./harness.mjs";

class Request {}
class Probe {
  str(v) {
    return v + "?";
  }
  reqs(v) {
    return [...v, new Request()];
  }
}
Object.assign(globalThis, { Request, Probe });

const { instantiate } = await import("./bridge.js");
const fs = await import("node:fs");
const compile = (f) => new WebAssembly.Module(fs.readFileSync(f));
const L = lang(instantiate(compile("lang.wasm"), compile("bridge.wasm")), { memory: "mem", "tw:free": "__free", "tw:table": "__table", "tw:table/const_base": "__const_base" });
assert.equal(L.ex.memory, undefined);
const p = new Probe();

// Every protocol export is reached through its renamed export.
L.balanced(() => assert.equal(L.takeStr(...L.call("Probe#str(str)", p, ...L.str("a"))), "a?"));
L.balanced(() => {
  const [base, len] = L.call("Probe#reqs(arr<Request>)", p, 0, 0);
  assert.equal(len, 1);
  assert.ok(L.ex.__table.get(base) instanceof Request);
  L.ex.__tfree(base, 1);
});

console.log("ok");
