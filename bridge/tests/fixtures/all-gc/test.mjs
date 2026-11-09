import { assert } from "./harness.mjs";

class Request {}
class ReadableStream {}
class Probe {
  fill(v) {
    v.fill(8, 1);
  }
}
const names = [
  "Str", "OptStr", "Bytes", "Floats", "Bigs", "OptBytes", "Nums", "U8s", "I8s", "Bools", "I16s", "U16s", "F16s", "I53s", "Strs", "OptStrs", "Rows", "Nested",
  "Mixed", "Pair", "OptPair", "Body", "OptBody", "Value", "Method", "Bodies",
];
for (const n of names) {
  Probe.prototype["make" + n] = function () {
    return this.next;
  };
  Probe.prototype["see" + n] = function (v) {
    this.seen = v;
  };
}
Object.assign(globalThis, { Request, ReadableStream, Probe });

const { instantiate, scratchUse } = await import("./bridge.js");
const fs = await import("node:fs");
const compile = (f) => new WebAssembly.Module(fs.readFileSync(f));
const ex = instantiate(compile("lang.wasm"), compile("bridge.wasm")).exports;
const keys = Object.keys(ex);
const key = (prefix) => {
  const k = keys.find((k) => k.startsWith(`call:Probe#${prefix}(`));
  assert.ok(k, prefix);
  return k;
};
const p = new Probe();
const base = scratchUse();

// JS → GC (make) then GC → JS (see): every conversion runs both ways.
const roundTrip = (n, v, ...rest) => {
  const expected = rest.length ? rest[0] : v;
  p.next = v;
  const g = ex[key("make" + n)](p);
  const w = Array.isArray(g) ? g : [g];
  p.seen = Symbol("unset");
  ex[key("see" + n)](p, ...w);
  assert.deepEqual(p.seen, expected, n);
  assert.deepEqual(scratchUse(), base, `${n}: scratch restored`);
  return w;
};
const fails = (n, v, re) => {
  p.next = v;
  assert.throws(() => ex[key("make" + n)](p), (e) => e instanceof TypeError && (!re || re.test(e.message)), n);
  assert.deepEqual(scratchUse(), base, `${n}: scratch restored after failure`);
};

for (const s of ["", "a", "héllo", "😀", "x".repeat(70000)]) roundTrip("Str", s);
roundTrip("Str", "a\ud800", "a�");
fails("Str", 5, /str/);
roundTrip("OptStr", "s");
roundTrip("OptStr", null, undefined);
assert.equal(roundTrip("OptStr", undefined)[0], null);

roundTrip("Bytes", new Uint8Array([1, 2, 255]));
roundTrip("Bytes", new Uint8Array(0));
roundTrip("Floats", new Float64Array([1.5, NaN, -0]));
roundTrip("Bigs", new BigInt64Array([-1n, 2n ** 63n - 1n]));
fails("Bytes", [1], /u8\[\]/);
roundTrip("OptBytes", null, undefined);
roundTrip("OptBytes", new Uint8Array([3]));

roundTrip("Nums", [0, 4294967295]);
roundTrip("Nums", []);
fails("Nums", [1, -1], /u32/);
// Scalar elements are stored as `T[]`'s are: `arr<u8>` is `(array (mut i8))`.
roundTrip("U8s", [0, 1, 255]);
fails("U8s", [256], /u8/);
roundTrip("I8s", [-128, -1, 127]);
roundTrip("Bools", [true, false, true]);
roundTrip("I16s", [-32768, 32767]);
roundTrip("U16s", [0, 65535]);
roundTrip("F16s", [1.5, -0, Infinity, 65504]);
roundTrip("I53s", [-(2 ** 53 - 1), 0, 2 ** 53 - 1]);
roundTrip("Strs", ["a", "", "ü"]);
fails("Strs", ["a", "b", 3], /str/);
roundTrip("OptStrs", ["x", null], ["x", undefined]);
roundTrip("OptStrs", null, undefined);
const r1 = new Request();
roundTrip("Rows", [[1, "a", r1], [2, "b", r1]]);
fails("Rows", [[1, "a", {}]], /Request/);
roundTrip("Nested", [["a"], [], ["b", "c"]]);
fails("Nested", [["a"], [1]]);
roundTrip("Mixed", [[true, null, 1], [false, -5, 2.5]], [[true, undefined, 1], [false, -5, 2.5]]);

roundTrip("Pair", [3, "p"]);
fails("Pair", [3]);
roundTrip("OptPair", [1, "o"]);
roundTrip("OptPair", null, undefined);

roundTrip("Body", "text");
roundTrip("Body", new Uint8Array([9]));
const s = new ReadableStream();
roundTrip("Body", s);
fails("Body", 5, /Body/);
roundTrip("OptBody", null, undefined);
roundTrip("OptBody", "t");
roundTrip("Value", 2.5);
roundTrip("Value", [7, "seven"]);
roundTrip("Value", ["a", "b", "c"]);
fails("Value", ["a", "b"], /u32/);
roundTrip("Value", { other: 1 });
roundTrip("Method", "POST");
fails("Method", "PUT", /Method/);
roundTrip("Bodies", ["t", new Uint8Array([1]), s]);

// Reentry mid-conversion: an element getter calls back into the language
// (a nested forwarder frame), then the outer conversion fails or the inner
// failure unwinds through both frames. Scratch is restored either way.
{
  const reenter = (inner) => {
    const a = ["a"];
    Object.defineProperty(a, 1, {
      get() {
        p.next = inner;
        ex[key("makeStr")](p);
        throw new SyntaxError("outer");
      },
    });
    return a;
  };
  p.next = reenter("x".repeat(1000));
  assert.throws(() => ex[key("makeStrs")](p), SyntaxError);
  assert.deepEqual(scratchUse(), base);
  p.next = reenter(5);
  assert.throws(() => ex[key("makeStrs")](p), TypeError);
  assert.deepEqual(scratchUse(), base);
}

// mut u8[]: the GC array is written back.
{
  p.next = new Uint8Array([1, 2, 3]);
  const g = ex[key("makeBytes")](p);
  ex[key("fill")](p, g);
  ex[key("seeBytes")](p, g);
  assert.deepEqual([...p.seen], [1, 8, 8]);
  assert.deepEqual(scratchUse(), base);
}

console.log("ok");
