import { case_, caseAsync, compile, profile } from "./lib.mjs";

const S8 = "abcdefgh";
const S1K = "x".repeat(1024);
const S1M = "y".repeat(1 << 20);
const SU1K = "é".repeat(512);
const B16 = new Uint8Array(16).fill(7);
const B64K = new Uint8Array(64 << 10).fill(7);
const B4M = new Uint8Array(4 << 20).fill(7);
const F1K = Array.from({ length: 1000 }, (_, i) => i + 0.5);
const STRS1K = Array.from({ length: 1000 }, (_, i) => "s" + (1000000 + i));
const ROWS1K = Array.from({ length: 1000 }, (_, i) => [i, "r" + i]);

class Req {}
const req = new Req();
let next = null;
class Probe {
  noop() {}
  id(v) { return v; }
  idF(v) { return v; }
  optF(v) { return v; }
  optU8(v) { return v; }
  seeStr() {}
  makeStr() { return next; }
  seeBytes() {}
  makeBytes() { return next; }
  seeF64s() {}
  makeF64s() { return next; }
  seeStrs() {}
  makeStrs() { return next; }
  seeRows() {}
  makeRows() { return next; }
  put() {}
  makeReq() { return req; }
  send() {}
  risky(v) { return v; }
  each(cb, n) { for (let i = 0; i < n; i++) cb(i); }
  pair() { return [1, 2.5]; }
  seeBox() {}
  makeBox() { return next; }
  later() { return Promise.resolve(7); }
}
globalThis.Probe = Probe;
globalThis.Req = Req;
globalThis.Map = Map;

const impl = {
  "util.id(i32)": (v) => v,
  "util.len(str)": () => 0,
  "tw:call:fn(i32)->i32": (_env, v) => v,
  "tw:drop:fn(i32)->i32": () => {},
  "tw:future/wake": () => {},
};
const { instantiate, util } = await import("./bridge.js");
const inst = instantiate(compile("lang.wasm"), compile("bridge.wasm", true), { "test:impl": impl });
const ex = inst.exports;
const p = new Probe();
const call = (k, ...a) => ex["call:" + k](...a);
const loop = (k) => {
  const f = ex["loop:" + k];
  if (!f) throw new Error("no loop for " + k);
  return f;
};
const wire = (v) => (Array.isArray(v) ? v : [v]);
const make = (k, v) => {
  next = v;
  return wire(call(k, p));
};
const base = profile.replace(/-single$/, "");
const linear = base === "linear";
// Results that allocate in the language's memory rewind it after each batch.
const MEM_BUDGET = 128 << 20;
const batched = (f, bytes) => (k) => {
  if (!linear) return f(k);
  const per = Math.max(1, Math.floor(MEM_BUDGET / Math.max(bytes, 64)));
  for (let done = 0; done < k; done += per) {
    f(Math.min(per, k - done));
    ex["bench:reset"]();
  }
};
ex["bench:mark"]();
const opsFor = (bytes) => Math.max(20, Math.min(2_000_000, Math.floor(4e8 / Math.max(bytes, 200))));

// Wasm → JS calls.
case_("noop()", 2e6, (k) => loop("Probe#noop()")(k, p));
case_("id(i32): i32", 2e6, (k) => loop("Probe#id(i32)")(k, p, 7));
case_("idF(f64): f64", 2e6, (k) => loop("Probe#idF(f64)")(k, p, 7.5));
{
  const f = loop("Probe#optF(f64?)");
  case_("optF(f64?): f64? (pair)", 2e6, (k) => f(k, p, 1, 7.5));
}
case_("optU8(u8?): u8? (niche)", 2e6, (k) => loop("Probe#optU8(u8?)")(k, p, 7));
case_("risky(i32): i32 throws (ok path)", 2e6, (k) => loop("Probe#risky(i32)")(k, p, 7));
case_("pair(): (u32, f64)", 2e6, (k) => loop("Probe#pair()")(k, p));
case_("put(Req, Req) pass-through", 2e6, (k) => loop("Probe#put(Req,Req)")(k, p, req, req));
case_("makeReq(): Req", 2e6, (k) => loop("Probe#makeReq()")(k, p));

for (const [label, s] of [["8", S8], ["1K", S1K], ["1K-utf8", SU1K], ["1M", S1M]]) {
  const w = make("Probe#makeStr()", s);
  case_(`seeStr(str ${label})`, opsFor(s.length), (k) => loop("Probe#seeStr(str)")(k, p, ...w));
  next = s;
  case_(`makeStr(): str ${label}`, opsFor(s.length), batched((k) => loop("Probe#makeStr()")(k, p), s.length * 3));
}
for (const [label, b] of [["16", B16], ["64K", B64K], ["4M", B4M]]) {
  const w = make("Probe#makeBytes()", b);
  case_(`seeBytes(u8[] ${label})`, opsFor(b.length), (k) => loop("Probe#seeBytes(u8[])")(k, p, ...w));
  next = b;
  case_(`makeBytes(): u8[] ${label}`, opsFor(b.length), batched((k) => loop("Probe#makeBytes()")(k, p), b.length));
}
{
  const w = make("Probe#makeF64s()", F1K);
  case_("seeF64s(arr<f64> 1000)", 20000, (k) => loop("Probe#seeF64s(arr<f64>)")(k, p, ...w));
  next = F1K;
  case_("makeF64s(): arr<f64> 1000", 20000, batched((k) => loop("Probe#makeF64s()")(k, p), 8000));
}
{
  const w = make("Probe#makeStrs()", STRS1K);
  case_("seeStrs(arr<str> 1000)", 5000, (k) => loop("Probe#seeStrs(arr<str>)")(k, p, ...w));
  next = STRS1K;
  case_("makeStrs(): arr<str> 1000", 5000, batched((k) => loop("Probe#makeStrs()")(k, p), 24000));
}
{
  const w = make("Probe#makeRows()", ROWS1K);
  case_("seeRows(arr<(u32,str)> 1000)", 5000, (k) => loop("Probe#seeRows(arr<(u32,str)>)")(k, p, ...w));
  next = ROWS1K;
  case_("makeRows(): arr<(u32,str)> 1000", 5000, batched((k) => loop("Probe#makeRows()")(k, p), 24000));
}

// Payload enum: the Text arm (a str) and the Stream arm (a host ref).
{
  const s = make("Probe#makeStr()", S8);
  const text = {
    linear: [1, ...s, null],
    gc: [1, s[0], null],
    "gc-wtf16": [1, s[0], null, null],
    "gc-hoststr": [1, s[0], null],
    host: [1, s[0]],
  }[base];
  const stream = {
    linear: [3, 0, 0, req],
    gc: [3, null, req],
    "gc-wtf16": [3, null, null, req],
    "gc-hoststr": [3, req, null],
    host: [3, req],
  }[base];
  case_("send(Body.Text(str 8))", 1e6, (k) => loop("Probe#send(Body)")(k, p, ...text));
  case_("send(Body.Stream(Req))", 1e6, (k) => loop("Probe#send(Body)")(k, p, ...stream));
}

// Boxes.
{
  const s = make("Probe#makeStr()", S8);
  case_("box str 8", 1e6, batched((k) => loop("box:str")(k, ...s), 8));
  case_("unbox str 8", 1e6, batched((k) => loop("unbox:str")(k, S8), 8));
  case_("box u8", 2e6, (k) => loop("box:u8")(k, 7));
  case_("unbox u8", 2e6, (k) => loop("unbox:u8")(k, 7));
  case_("box f64 (3.5)", 2e6, (k) => loop("box:f64")(k, 3.5));
  case_("unbox f64 (3.5)", 2e6, (k) => loop("unbox:f64")(k, 3.5));
  const m = new Map();
  case_("erased Map#set(K,V) boxes", 1e6, (k) => loop("Map#set(K,V)")(k, m, "k", 7));
  m.set("k", 7);
  case_("erased Map#get(K)", 1e6, (k) => loop("Map#get(K)")(k, m, "k"));
}

// Callbacks: JS calls a language function n times through its wrapper.
{
  const env = linear ? 4096 : {};
  case_("callback fn(i32)->i32 from JS", 1e6, (k) => call("Probe#each(fn(i32)->i32,u32)", p, env, k));
}

// JS → Wasm exports through the bridge facade.
case_("export util.id(i32)", 2e6, (k) => {
  for (let i = 0; i < k; i++) util.id(i);
});
case_("export util.len(str 8)", 1e6, batched((k) => {
  for (let i = 0; i < k; i++) util.len(S8);
}, 8));
case_("export util.len(str 1K)", 2e5, batched((k) => {
  for (let i = 0; i < k; i++) util.len(S1K);
}, 1024));

// A future round trip: the import makes it, the promise settles it, and the
// language polls, takes and drops it.
{
  const fs = new Array(10000);
  await caseAsync("future<u32> make, settle, poll, take, drop", 1e5, async (k) => {
    for (let done = 0; done < k; done += fs.length) {
      const n = Math.min(fs.length, k - done);
      for (let i = 0; i < n; i++) {
        fs[i] = call("Probe#later()", p);
        call("future:poll", fs[i], 1);
      }
      await new Promise((r) => setImmediate(r));
      for (let i = 0; i < n; i++) {
        if (call("future:poll", fs[i], 1) !== 1) throw new Error("unsettled");
        call("future:take<u32>", fs[i]);
        call("future:drop", fs[i]);
      }
    }
  });
}

// The floor: the same shapes with no bridge.
if (profile === "linear") {
  const fs = await import("node:fs");
  const b = new WebAssembly.Instance(new WebAssembly.Module(fs.readFileSync("baseline.wasm")), {
    b: {
      noop: () => {},
      id: (_r, v) => v,
      idcall: Function.prototype.call.bind(Probe.prototype.id),
      put: () => {},
      pair: () => [1, 2.5],
      cb: (_e, v) => v,
    },
  }).exports;
  const save = process.env.BENCH_PROFILE;
  case_("FLOOR noop import", 2e6, (k) => b["loop:noop"](k, p));
  case_("FLOOR id import (arrow)", 2e6, (k) => b["loop:id"](k, p, 7));
  case_("FLOOR id import (call.bind method)", 2e6, (k) => b["loop:idcall"](k, p, 7));
  case_("FLOOR put import", 2e6, (k) => b["loop:put"](k, p, req, req));
  case_("FLOOR pair import (multi-value)", 2e6, (k) => b["loop:pair"](k, p));
  case_("FLOOR JS→Wasm export id", 2e6, (k) => {
    for (let i = 0; i < k; i++) b.id(i);
  });
  case_("FLOOR JS→Wasm→JS cb", 1e6, (k) => {
    for (let i = 0; i < k; i++) b.cbExport(0, i);
  });
}
