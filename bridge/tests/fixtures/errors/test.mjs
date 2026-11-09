import { assert, lang } from "./harness.mjs";

class Kv {
  constructor() {
    this.next = undefined;
  }
}
for (const m of ["get", "plain", "raw", "list", "count"]) {
  Kv.prototype[m] = function () {
    if (typeof this.next === "function") return this.next();
    return this.next;
  };
}
globalThis.Kv = Kv;

const { instantiate } = await import("./bridge.js");
const fs = await import("node:fs");
const compile = (f) => new WebAssembly.Module(fs.readFileSync(f));
const L = lang(instantiate(compile("lang.wasm"), compile("bridge.wasm")));
const kvTag = L.ex["tag:KvError"];
const errTag = L.ex["tag:err"];
const kv = new Kv();

// Calls `key` with a host that throws `thrown`; returns the language-side
// tag payload `[arm, message]`.
const raise = (key, tag, thrown, ...wire) => {
  kv.next = () => {
    throw thrown;
  };
  let caught;
  L.balanced(() => {
    try {
      L.call(key, kv, ...wire);
    } catch (e) {
      caught = e;
    }
  });
  assert.ok(caught instanceof WebAssembly.Exception, `${key}: ${caught}`);
  assert.ok(caught.is(tag));
  return [caught.getArg(tag, 0), caught.getArg(tag, 1)];
};
const get = (thrown) => raise("Kv#get(str)", kvTag, thrown, ...L.str("k"));

// Discrimination: child arms first, then the base arms, then the fallback.
assert.deepEqual(get(new DOMException("missing", "NotFoundError")), [2, "missing"]);
assert.deepEqual(get(new DOMException("other", "AbortError")), [-1, "other"]);
assert.deepEqual(get(Object.assign(new RangeError("coded"), { code: 7 })), [3, "coded"]);
assert.deepEqual(get(new RangeError("range")), [1, "range"]);
assert.deepEqual(get(new TypeError("type")), [1, "type"]);
assert.deepEqual(get(new Error("plain")), [-1, "plain"]);

// Message extraction: message, then String(err), then a constant, guarded.
assert.deepEqual(get(42), [-1, "42"]);
assert.deepEqual(get({}), [-1, "[object Object]"]);
assert.deepEqual(get(""), [-1, "unknown error"]);
assert.deepEqual(get(Symbol("s")), [-1, "Symbol(s)"]);
assert.deepEqual(
  get({
    toString() {
      throw new Error("nope");
    },
  }),
  [-1, "unknown error"],
);
assert.deepEqual(
  get({
    get message() {
      throw new Error("nope");
    },
  }),
  [-1, "[object Object]"],
);
assert.deepEqual(get(null), [-1, "null"]);

// The built-in err: RangeError lands on TypeError.
const plain = (thrown) => raise("Kv#plain(str)", errTag, thrown, ...L.str("k"));
assert.deepEqual(plain(new RangeError("r")), [1, "r"]);
assert.deepEqual(plain(new DOMException("m", "NotFoundError")), [-1, "m"]);

// Validation failures inside a throws import are TypeErrors, discriminated
// like any other error, and release the partial result first.
{
  kv.next = ["a", "b", 5];
  let caught;
  L.balanced(() => {
    try {
      L.call("Kv#list()", kv);
    } catch (e) {
      caught = e;
    }
  });
  assert.ok(caught.is(errTag));
  assert.equal(caught.getArg(errTag, 0), 1);
  assert.match(caught.getArg(errTag, 1), /expected str/);
}
{
  kv.next = 256;
  let caught;
  try {
    L.call("Kv#count()", kv);
  } catch (e) {
    caught = e;
  }
  assert.equal(caught.getArg(kvTag, 0), 1);
  assert.match(caught.getArg(kvTag, 1), /u8/);
}

// A WebAssembly.Exception (a foreign tag) passes through untouched.
{
  const foreign = new WebAssembly.Tag({ parameters: ["i32"] });
  const ex = new WebAssembly.Exception(foreign, [7]);
  kv.next = () => {
    throw ex;
  };
  assert.throws(
    () => L.call("Kv#get(str)", kv, ...L.str("k")),
    (e) => e === ex,
  );
}

// No throws: the original value unwinds through the language, identity kept.
{
  const err = new SyntaxError("raw");
  kv.next = () => {
    throw err;
  };
  L.balanced(() =>
    assert.throws(
      () => L.call("Kv#raw(str)", kv, ...L.str("k")),
      (e) => e === err,
    ),
  );
  kv.next = 5;
  L.balanced(() => L.throwsType(() => L.call("Kv#raw(str)", kv, ...L.str("k")), /expected str/));
}

// Success still returns normally.
kv.next = "value";
assert.equal(L.takeStr(...L.call("Kv#get(str)", kv, ...L.str("k"))), "value");

console.log("ok");
