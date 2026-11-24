import { assert, lang } from "./harness.mjs";

class Present {}
let next;
const ret = () => next;
Object.assign(globalThis, { Present, absents: ret, presents: ret });

const { instantiate } = await import("./bridge.js");
const fs = await import("node:fs");
const compile = (f) => new WebAssembly.Module(fs.readFileSync(f));
const L = lang(instantiate(compile("lang.wasm"), compile("bridge.wasm")));
const call = (key, v) => {
  next = v;
  return L.call(key);
};

// A host array's elements check as a class with no value does: any
// non-null object.
const o = {};
call("absents()", [o, new Present()]);
L.throwsType(() => call("absents()", [o, 5]), /Absent/);
L.throwsType(() => call("absents()", [null]), /Absent/);
// A present class still checks nominally.
call("presents()", [new Present()]);
L.throwsType(() => call("presents()", [o]), /Present/);

console.log("ok");
