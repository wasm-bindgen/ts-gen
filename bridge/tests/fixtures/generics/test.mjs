import { assert, lang } from "./harness.mjs";

class Util {
  static first(xs) {
    return xs[0];
  }
  static pair(a, b) {
    return [a, b];
  }
}
class Bag {
  put(v) {
    this.got = v;
  }
}
class Shape {
  static read(key) {
    return { width: 7, label: "sq" }[key];
  }
}
Object.assign(globalThis, { Util, Bag, Shape });

const { instantiate } = await import("./bridge.js");
const fs = await import("node:fs");
const compile = (f) => new WebAssembly.Module(fs.readFileSync(f));
const L = lang(instantiate(compile("lang.wasm"), compile("bridge.wasm")));

// Mono variants convert through their type arguments.
const m = L.call("Map.new<str,u32>()");
assert.ok(m instanceof Map);
assert.equal(L.call("Map#set<str,u32>(K,V)", m, ...L.str("a"), 7), m);
assert.deepEqual(L.call("Map#get<str,u32>(K)", m, ...L.str("a")), [1, 7]);
assert.deepEqual(L.call("Map#get<str,u32>(K)", m, ...L.str("b")), [0, 0]);
m.set("c", "nope");
L.throwsType(() => L.call("Map#get<str,u32>(K)", m, ...L.str("c")), /u32/);

// Erased: every `T` is its JS value.
assert.equal(L.call("Map#set(K,V)", m, "d", { x: 1 }), m);
const d = L.call("Map#get(K)", m, "d");
assert.deepEqual(d, { x: 1 });
assert.equal(L.call("Map#get(K)", m, "a"), 7);
// An erased empty enters as ref.null.
assert.equal(L.call("Map#get(K)", m, "zz"), null);

const xs = L.bytes(new Uint32Array([5, 6]), 4);
assert.deepEqual(L.call("Util.first<u32>(arr<T>)", xs, 2), [1, 5]);
const [p0, p1, flag] = L.call("Util.pair<str,bool>(A,B)", ...L.str("s"), 1);
assert.equal(L.takeStr(p0, p1), "s");
assert.equal(flag, 1);
const xb = L.ex["tw:table/alloc"](2);
L.ex["tw:table"].set(xb, "e0");
L.ex["tw:table"].set(xb + 1, 9);
assert.equal(L.call("Util.first(arr<T>)", xb, 2), "e0");

const b = new Bag();
L.call("Bag#put<str>(T)", b, ...L.str("x"));
assert.equal(b.got, "x");
console.log("done");

// A key's variant supplies it, and reads its own property's type.
assert.equal(L.call('Shape.read<"width">(K)'), 7);
assert.equal(L.takeStr(...L.call('Shape.read<"label">(K)')), "sq");
// Erased, a key is a string and the result any of the properties.
assert.equal(L.call("Shape.read(K)", ...L.str("width")), 7);
assert.equal(L.call("Shape.read(K)", ...L.str("label")), "sq");
// A variant may leave its key a value.
assert.equal(L.call("Shape.read<str>(K)", ...L.str("label")), "sq");
