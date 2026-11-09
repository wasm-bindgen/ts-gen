import { assert, lang } from "./harness.mjs";

class Request {}
class Probe {
  take(v) {
    return v;
  }
  opt(v) {
    return v;
  }
  arr(v) {
    return v;
  }
  tup(v) {
    return v;
  }
  num(v) {
    return v;
  }
  nopt(v) {
    this.seen = v;
  }
  nested(v) {
    this.seen = v;
    return v;
  }
}
Object.assign(globalThis, { Request, Probe });

const { instantiate } = await import("./bridge.js");
const fs = await import("node:fs");
const compile = (f) => new WebAssembly.Module(fs.readFileSync(f));
const L = lang(instantiate(compile("lang.wasm"), compile("bridge.wasm")));
const p = new Probe();

// Box positions are lazy: the value crosses untouched, even an empty one,
// and is type-tested only on unbox.
for (const v of ["s", 5, {}, undefined, null]) {
  assert.equal(L.call("Probe#take(Str)", p, v), v);
  assert.equal(L.call("Probe#opt(Str?)", p, v), v);
}
L.balanced(() => L.throwsType(() => L.call("unbox:str", L.call("Probe#take(Str)", p, undefined)), /str/));

// A `T?` box is none on either empty, which tw:unbox reads as none, and a
// `T??` box's outer none is undefined.
for (const [v, nullish, undef] of [
  [undefined, 1, 1],
  [null, 1, 0],
  [0, 0, 0],
  ["", 0, 0],
  [false, 0, 0],
  [{}, 0, 0],
]) {
  assert.equal(L.call("isNullish:", v), nullish);
  assert.equal(L.call("isUndefined:", v), undef);
}
assert.deepEqual(L.call("unbox:str?", undefined), L.call("unbox:str?", null));
L.balanced(() => assert.equal(L.takeStr(...L.call("unbox:str?", "q")), "q"));
assert.equal(L.call("unbox:Request?", undefined), null);
assert.equal(L.call("unboxUnchecked:Request?", undefined), null);

// Under `box-none = unit`, the unit box is a box's none, and the Bool
// boxes are constants.
const unit = L.call("unit:");
assert.equal(unit, undefined);
assert.equal(L.call("true:"), true);
assert.equal(L.call("false:"), false);
assert.equal(L.call("box:bool?", 2), undefined);
assert.equal(L.call("box:bool?", 1), true);

// A box crosses faithfully, except that a `@js(null) T?` writes unit as
// null. A received null passed back stays null, a `T??` box keeps both
// nones, and an erased position stores unit as undefined.
for (const [v, seen] of [
  [unit, null],
  [null, null],
  ["s", "s"],
]) {
  L.call("Probe#nopt(Str?)", p, v);
  assert.equal(p.seen, seen);
}
for (const v of [unit, null, "s"]) {
  assert.equal(L.call("Probe#nested(Str??)", p, v), v);
  assert.equal(p.seen, v);
}
{
  const m = new Map();
  L.call("Map#set(K,V)", m, "k", unit);
  assert.ok(m.has("k"));
  assert.equal(m.get("k"), undefined);
}

// tw:box copies out of the language's carrier.
L.balanced(() => assert.equal(L.call("box:str", ...L.str("héllo")), "héllo"));
L.balanced(() => assert.deepEqual(L.call("box:arr<u8>", L.bytes(new Uint8Array([1, 255])), 2), [1, 255]));
L.balanced(() => assert.deepEqual(L.call("box:(u32,str)", 7, ...L.str("x")), [7, "x"]));
{
  const ptr = L.bytes(new Uint8Array([9]));
  const v = L.call("box:u8[]", ptr, 1);
  assert.ok(v instanceof Uint8Array);
  assert.notEqual(v.buffer, L.ex.memory.buffer);
}
assert.equal(L.call("box:Method", 2), "POST");
assert.equal(L.call("box:i64", -1n), -1n);

// tw:unbox type-tests, then copies and validates.
L.balanced(() => assert.equal(L.takeStr(...L.call("unbox:str", "abc")), "abc"));
L.balanced(() => L.throwsType(() => L.call("unbox:str", 5), /str/));
L.balanced(() => assert.deepEqual([...L.take(...L.call("unbox:u8[]", new Uint8Array([4])))], [4]));
L.balanced(() => L.throwsType(() => L.call("unbox:u8[]", [4])));
L.balanced(() => {
  const [ptr, len] = L.call("unbox:arr<u16>", [1, 65535]);
  assert.deepEqual([...new Uint16Array(L.take(ptr, len, 2, 2).buffer)], [1, 65535]);
});
L.balanced(() => L.throwsType(() => L.call("unbox:arr<u16>", [1, 65536]), /u16/));
L.balanced(() => {
  const [n, sp, sl] = L.call("unbox:(u32,str)", [3, "t"]);
  assert.equal(n, 3);
  assert.equal(L.takeStr(sp, sl), "t");
});
L.balanced(() => L.throwsType(() => L.call("unbox:(u32,str)", [3, 4])));
assert.equal(L.call("unbox:Method", "GET"), 1);
L.throwsType(() => L.call("unbox:Method", "PUT"));
{
  const r = new Request();
  assert.equal(L.call("unbox:Request", r), r);
  L.throwsType(() => L.call("unbox:Request", {}), /Request/);
}
assert.ok(Object.is(L.call("unbox:f64", -0), -0));
L.throwsType(() => L.call("unbox:f64", "1"));
assert.equal(L.call("unbox:i53", 2 ** 53 - 1), 2n ** 53n - 1n);
L.throwsType(() => L.call("unbox:i53", 2 ** 53));

// tw:unboxUnchecked skips the type test but stays memory safe.
L.balanced(() => assert.equal(L.takeStr(...L.call("unboxUnchecked:str", 12)), "12"));
assert.equal(L.call("unboxUnchecked:u8", 300), 44);

// tw:arr reads one element at a time.
{
  const a = ["a", "bb"];
  assert.equal(L.call("arr:len", a), 2);
  L.balanced(() => assert.equal(L.takeStr(...L.call("arr:get<str>", a, 1)), "bb"));
  L.throwsType(() => L.call("arr:get<str>", a, 2), /range/);
  L.throwsType(() => L.call("arr:get<str>", [5], 0), /str/);
  L.throwsType(() => L.call("arr:len", "ab"), /Arr/);
  assert.equal(L.call("arr:get<u16>", [1, 2], 1), 2);
  const r = new Request();
  assert.equal(L.call("arr:get<Request>", [r], 0), r);
  // A box element type returns the element itself, unconverted.
  const inner = [1, 2];
  assert.equal(L.call("arr:get<Arr<u8>>", [inner], 0), inner);
  assert.equal(L.call("arr:get<U16>", [1, 70000], 1), 70000);
  assert.equal(L.call("arr:get<Str>", a, 0), "a");
  assert.equal(L.call("arr:get<Str>", [undefined], 0), undefined);
  assert.equal(L.call("arr:get<Str?>", [undefined], 0), null);
  assert.equal(L.call("arr:get<Str?>", a, 1), "bb");
}

// `bigint` crosses as two's-complement little-endian `u64` limbs, the
// fewest that keep its sign.
for (const [v, n] of [
  [0n, 1],
  [-1n, 1],
  [2n ** 63n, 2],
  [2n ** 64n, 2],
  [-(2n ** 200n), 4],
]) {
  L.balanced(() => {
    const [p, len] = L.call("unbox:bigint", v);
    assert.equal(Number(len), n);
    assert.equal(L.call("box:bigint", p, len), v);
    L.take(p, len, 8, 8);
  });
}
L.throwsType(() => L.call("unbox:bigint", 1), /bigint/);
assert.deepEqual(L.call("unbox:bigint?", undefined), L.call("unbox:bigint?", null));
assert.equal(L.call("box:BigInt?", 5n), 5n);

// tw:instanceof.
assert.equal(L.call("instanceof:Request", new Request()), 1);
assert.equal(L.call("instanceof:Request", {}), 0);
assert.equal(L.call("instanceof:Request", null), 0);

console.log("ok");
