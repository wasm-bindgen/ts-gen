import { assert, lang } from "./harness.mjs";

class Request {}
class Probe {}
for (const m of ["u8", "f64", "str", "bytes", "nums", "pair", "method", "request", "maybe"]) {
  Probe.prototype[m] = function () {
    return this.next;
  };
}
const Keys = { echo: () => Keys.next };
Object.assign(globalThis, { Request, Probe, Keys });

const { instantiate, app } = await import("./bridge.js");
const fs = await import("node:fs");
const compile = (f) => new WebAssembly.Module(fs.readFileSync(f));
let L;
const impl = {
  "app.u8(u8)": (x) => x,
  "app.f64(f64)": (x) => x,
  "app.len(str)": (p, n) => (L.take(p, n), n),
  "app.bytes(u8[])": (p, n) => (L.take(p, n), n),
  "app.nums(arr<u32>)": (p, n) => (L.take(p, n, 4, 4), n),
  "app.pair((u32,str))": (x, p, n) => (L.take(p, n), x),
  "app.method(Method)": (m) => m,
  "app.request(Request)": () => 1,
  "app.maybe(Request?)": (r) => (r === null ? 0 : 1),
  'app.pick<"width">(K)': () => 7,
};
L = lang(instantiate(compile("lang.wasm"), compile("bridge.wasm"), { "test:impl": impl }));
// The receiver is user code's own object: its results are still an
// import's.
const p = new Probe();
const call = (key, next) => {
  p.next = next;
  return L.call(key, p);
};

// The same wrong values: trusted as an import's results, a `TypeError` as
// an export's arguments.
assert.equal(call("Probe#u8()", 300), 44);
L.throwsType(() => app.u8(300), /u8/);
assert.equal(call("Probe#f64()", "2.5"), 2.5);
L.throwsType(() => app.f64("2.5"), /f64/);
L.balanced(() => assert.equal(L.takeStr(...call("Probe#str()", 42)), "42"));
L.balanced(() => L.throwsType(() => app.len(42), /str/));
L.balanced(() => assert.deepEqual([...L.take(...call("Probe#bytes()", [1, 2]))], [1, 2]));
L.balanced(() => L.throwsType(() => app.bytes([1, 2]), /u8\[\]/));
L.balanced(() => {
  const [ptr, n] = call("Probe#nums()", { length: 2, 0: 7, 1: "8" });
  assert.deepEqual([...new Uint32Array(L.take(ptr, n, 4, 4).buffer)], [7, 8]);
});
L.balanced(() => L.throwsType(() => app.nums({ length: 2, 0: 7, 1: 8 })));
L.balanced(() => L.throwsType(() => app.nums([7, "8"]), /u32/));
L.balanced(() => {
  const [n, sp, sl] = call("Probe#pair()", [1, "a", "more"]);
  assert.equal(n, 1);
  assert.equal(L.takeStr(sp, sl), "a");
});
L.balanced(() => L.throwsType(() => app.pair([1, "a", "more"])));
{
  const o = {};
  assert.equal(call("Probe#request()", o), o);
  L.throwsType(() => app.request(o), /Request/);
}

// The right ones cross as they do validated.
assert.equal(call("Probe#u8()", 7), 7);
assert.equal(app.u8(7), 7);
assert.equal(L.balanced(() => app.len("abc")), 3);
assert.equal(L.balanced(() => app.bytes(new Uint8Array([1, 2]))), 2);
assert.equal(L.balanced(() => app.nums([7, 8])), 2);
assert.equal(L.balanced(() => app.pair([5, "a"])), 5);
assert.equal(app.request(new Request()), true);

// A nullable host value reads either empty as none.
assert.equal(call("Probe#maybe()", undefined), null);
assert.equal(call("Probe#maybe()", null), null);
assert.equal(app.maybe(undefined), false);
assert.equal(app.maybe(new Request()), true);

// A literal result isn't compared. One an export takes is.
Keys.next = "height";
L.call('Keys.echo<"width">(K)');
assert.equal(app.pick("width"), 7);
L.throwsType(() => app.pick("height"), /width/);

// Dispatch runs on both sides.
assert.equal(call("Probe#method()", "POST"), 2);
L.throwsType(() => call("Probe#method()", "PUT"), /Method/);
assert.equal(app.method("POST"), 2);
L.throwsType(() => app.method("PUT"), /Method/);

// `tw:unbox` and `tw:arr get` validate: `tw:unboxUnchecked` is the
// language's way to trust a box.
L.throwsType(() => L.call("unbox:u8", 300), /u8/);
L.throwsType(() => L.call("arr:get<u8>", [300], 0), /u8/);
assert.equal(L.call("unboxUnchecked:u8", 300), 44);

console.log("ok");
