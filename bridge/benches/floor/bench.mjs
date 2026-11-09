import fs from "node:fs";
import { case_ } from "./lib.mjs";
let inst;
const f64 = () => new Float64Array(inst.exports.mem.buffer);
let cache = null, cbuf = null;
const out = [0, 0];
let k_ = 0;
inst = new WebAssembly.Instance(new WebAssembly.Module(fs.readFileSync("baseline.wasm")), {
  b: {
    one: () => 7,
    two: () => [7, 8],
    pairf: () => [1, 2.5],
    viaMem: () => {
      const b = inst.exports.mem.buffer;
      if (b !== cbuf) { cbuf = b; cache = new Float64Array(b); }
      cache[1] = 2.5;
      return 1;
    },
    viaGlobal: () => {
      inst.exports.g.value = 2.5;
      return 1;
    },
    f64nan: () => 2.5,
    i64r: () => 7n,
    i64pack: () => {
      const p = 4096 + (k_++ & 1023), len = 8;
      return (BigInt(len) << 32n) | BigInt(p);
    },
    viaMemLen: () => {
      if (cache.length === 0) cache = new Float64Array(inst.exports.mem.buffer);
      cache[1] = 2.5;
      return 1;
    },
    f64pack: () => {
      const p = 4096 + (k_++ & 1023), len = 8;
      return len * 4294967296 + p;
    },
  },
});
const e = inst.exports;
case_("FLOOR result i32", 2e6, (k) => e["loop:one"](k));
case_("FLOOR result (i32,i32) array", 2e6, (k) => e["loop:two"](k));
case_("FLOOR result (i32,f64) array", 2e6, (k) => e["loop:pairf"](k));
case_("FLOOR result i32 + f64 via memory (buffer check)", 2e6, (k) => e["loop:viaMem"](k));
case_("FLOOR result i32 + f64 via Global.value", 2e6, (k) => e["loop:viaGlobal"](k));
case_("FLOOR result f64 (NaN-niche shape)", 2e6, (k) => e["loop:f64nan"](k));
case_("FLOOR result i64 (bigint)", 2e6, (k) => e["loop:i64r"](k));
cache = new Float64Array(e.mem.buffer);
case_("FLOOR result i32 + f64 via memory (detach check)", 2e6, (k) => e["loop:viaMemLen"](k));
case_("FLOOR result (ptr,len) packed in i64 bigint", 2e6, (k) => e["loop:i64pack"](k));
case_("FLOOR result (ptr,len) packed in f64", 2e6, (k) => e["loop:f64pack"](k));

{
  const x = inst.exports;
  x.mk(65536);
  case_("FLOOR GC→mem copy 64K, byte loop", 2000, (k) => { for (let i = 0; i < k; i++) x.ser1(); });
  case_("FLOOR GC→mem copy 64K, 4-byte words", 2000, (k) => { for (let i = 0; i < k; i++) x.ser4(); });
  case_("FLOOR mem→GC copy 64K, byte loop", 2000, (k) => { for (let i = 0; i < k; i++) x.de1(65536); });
  case_("FLOOR mem→GC copy 64K, 4-byte words", 2000, (k) => { for (let i = 0; i < k; i++) x.de4(65536); });
  const m = new Uint8Array(x.mem.buffer, 1024, 65536), d = new Uint8Array(65536);
  case_("FLOOR mem→mem copy 64K (set)", 2000, (k) => { for (let i = 0; i < k; i++) d.set(m); });
}

{
  const x = inst.exports;
  let s = 0;
  case_("FLOOR JS→Wasm export, 1 result", 2e6, (k) => { for (let i = 0; i < k; i++) s += x.one2(i); });
  case_("FLOOR JS→Wasm export, (i32, f64) result", 2e6, (k) => { for (let i = 0; i < k; i++) s += x.pair2(i)[0]; });
}
