import { assert, lang } from "./harness.mjs";

// `Date` comes from the std lib: a JS value like the IR's own classes, with
// no glue of its own here.
class Event {
  get stamp() {
    return this.next;
  }
  at(d) {
    return d.getTime();
  }
}
globalThis.Event = Event;

const { instantiate } = await import("./bridge.js");
const fs = await import("node:fs");
const compile = (f) => new WebAssembly.Module(fs.readFileSync(f));
const L = lang(instantiate(compile("lang.wasm"), compile("bridge.wasm")));
const e = new Event();
const d = new Date(5);
e.next = d;
assert.equal(L.call("Event#get:stamp()", e), d);
e.next = {};
L.throwsType(() => L.call("Event#get:stamp()", e), /Date/);
assert.equal(L.call("Event#at(Date)", e, d), 5);
console.log("done");
