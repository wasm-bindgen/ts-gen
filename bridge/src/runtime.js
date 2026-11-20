const $te = new TextEncoder();
const $td = new TextDecoder("utf-8", { ignoreBOM: true });

function $describe(v) {
  switch (typeof v) {
    case "string":
      return "a string";
    case "number":
      return `the number ${v}`;
    case "bigint":
      return `the bigint ${v}`;
    case "boolean":
      return `${v}`;
    case "undefined":
      return "undefined";
    case "symbol":
      return "a symbol";
    case "function":
      return "a function";
    default:
      return v === null ? "null" : "an object";
  }
}

function $fail(what, v) {
  throw new TypeError(`expected ${what}, got ${$describe(v)}`);
}

function $neverResolves() {
  throw new TypeError("a `never` future resolved");
}

function $int(v, lo, hi, what) {
  if (typeof v !== "number" || !Number.isInteger(v) || v < lo || v > hi) $fail(what, v);
  return v;
}

function $bigint(v, what) {
  if (typeof v !== "bigint") $fail(what, v);
  return v;
}

function $bool(v) {
  if (typeof v !== "boolean") $fail("bool", v);
  return v ? 1 : 0;
}

function $num(v, what) {
  if (typeof v !== "number") $fail(what, v);
  return v;
}

function $i53(v) {
  if (typeof v !== "number" || !Number.isSafeInteger(v)) $fail("i53", v);
  return BigInt(v);
}

// A 53-bit value from the language outside its range is the language's
// bug, a TypeError even with validation off. `i64::MIN` is the absent
// niche of `i53?` and `u53?`, read before these.
const $MAX53 = 2n ** 53n - 1n;
const $NICHE53 = -(2n ** 63n);

function $liftI53(w) {
  if (w > $MAX53 || w < -$MAX53) throw new TypeError(`the language produced ${w}, outside i53`);
  return Number(w);
}

function $liftU53(w) {
  if (w > $MAX53 || w < 0n) throw new TypeError(`the language produced ${w}, outside u53`);
  return Number(w);
}

function $u53(v) {
  if (typeof v !== "number" || !Number.isSafeInteger(v) || v < 0) $fail("u53", v);
  return BigInt(v);
}

function $i64(v) {
  if (typeof v === "bigint") {
    if (BigInt.asIntN(64, v) === v) return v;
  } else if (typeof v === "number" && Number.isInteger(v) && v >= -(2 ** 63) && v < 2 ** 63) {
    return BigInt(v);
  }
  $fail("i64", v);
}

function $u64(v) {
  if (typeof v === "bigint") {
    if (BigInt.asUintN(64, v) === v) return v;
  } else if (typeof v === "number" && Number.isInteger(v) && v >= 0 && v < 2 ** 64) {
    return BigInt(v);
  }
  $fail("u64", v);
}

// Unvalidated conversions stay memory safe: they only ever produce a
// value of the wire type.
function $uint(v) {
  return Number(v) || 0;
}

function $ubig(v) {
  if (typeof v === "bigint") return v;
  const n = Math.trunc(Number(v));
  return Number.isFinite(n) ? BigInt(n) : 0n;
}

function $ustr(v) {
  return typeof v === "string" ? v : String(v);
}

function $str(v) {
  if (typeof v !== "string") $fail("str", v);
  return v;
}

function $symbol(v, what) {
  if (typeof v !== "symbol") $fail(what, v);
  return v;
}

function $some(v, what) {
  if (v === null || v === undefined) $fail(what, v);
  return v;
}

// Whether `a` holds nothing past its first `n` arguments but `undefined`s.
function $upTo(a, n) {
  for (let i = n; i < a.length; i++) {
    if (a[i] !== undefined) return false;
  }
  return true;
}

// A class's value, or null when its path holds no function: its checks
// then degrade to the interface test, and its arm tests never match (§8).
function $class(C) {
  return typeof C === "function" ? C : null;
}

function $instance(v, C, what) {
  if (C === null ? !$isObject(v) : !(v instanceof C)) $fail(what, v);
  return v;
}

function $isInstance(v, C) {
  return typeof C === "function" && v instanceof C;
}

function $isObject(v) {
  return (typeof v === "object" && v !== null) || typeof v === "function";
}

function $isThenable(v) {
  return v !== null && v !== undefined && typeof v.then === "function";
}

function $array(v, what) {
  if (!Array.isArray(v)) $fail(what, v);
  return v;
}

function $tuple(v, n, what) {
  if (!Array.isArray(v) || v.length !== n) $fail(what, v);
  return v;
}

function $typed(v, C, what) {
  if (!(v instanceof C)) $fail(what, v);
  return v;
}

// `("tw:unbox", vec<u8>)` reads any buffer or view as its bytes.
function $bytes(v) {
  if (v instanceof ArrayBuffer) return new Uint8Array(v);
  if (ArrayBuffer.isView(v)) return new Uint8Array(v.buffer, v.byteOffset, v.byteLength);
  $fail("a buffer or view", v);
}

// The error message of a caught value (IR.md §15): `String(err)` runs
// user code, so it's guarded.
function $message(e) {
  try {
    const m = e !== null && e !== undefined ? e.message : undefined;
    if (typeof m === "string" && m !== "") return m;
  } catch {}
  try {
    const s = String(e);
    if (s !== "") return s;
  } catch {}
  return "unknown error";
}

// A memory and its externref table, as the glue reads and writes them.
// The language's space allocates through its exports and registers each
// allocation on the call's cleanup list; the scratch space is the
// bridge's own, bump allocated and reset by the bridge module.
class $Space {
  constructor(memory, table, wide) {
    this.memory = memory;
    this.table = table;
    this.wide = wide;
    this.buf = null;
    this.u8_ = new Uint8Array(0);
    this.grows =
      typeof SharedArrayBuffer === "function" && memory?.buffer instanceof SharedArrayBuffer;
  }

  // The `buffer` getter is costly, so the views are kept until a grow
  // detaches them. A shared memory's grow doesn't detach, so it checks.
  dv() {
    if (this.u8_.length === 0 || this.grows) {
      const b = this.memory.buffer;
      if (b !== this.buf || b.byteLength !== this.u8_.length) {
        this.buf = b;
        this.dv_ = new DataView(b);
        this.u8_ = new Uint8Array(b);
      }
    }
    return this.dv_;
  }

  u8() {
    this.dv();
    return this.u8_;
  }

  // A pointer-sized wire value.
  p(n) {
    return this.wide ? BigInt(n) : n;
  }
}

class $Lang extends $Space {
  constructor(exports, names, wide) {
    super(exports[names.memory], exports[names.table], wide);
    this.shared = this.grows;
    this.realloc_ = exports[names.realloc];
    this.free_ = exports[names.free];
    this.talloc_ = exports[names.talloc];
    this.tfree_ = exports[names.tfree];
  }

  realloc(old, oldSize, align, size) {
    const p = this.realloc_(this.p(old), this.p(oldSize), this.p(align), this.p(size));
    return Number(p);
  }

  alloc(size, align, cx) {
    const p = this.realloc(0, 0, align, size);
    if (cx) $cxPush(cx, 0, p, size, align);
    return p;
  }

  free(p, size, align) {
    this.free_(this.p(p), this.p(size), this.p(align));
  }

  talloc(n, cx) {
    const b = this.talloc_(n) >>> 0;
    if (cx) $cxPush(cx, 1, b, n, 0);
    return b;
  }

  tfree(b, n) {
    for (let i = 0; i < n; i++) this.table.set(b + i, null);
    this.tfree_(b, n);
  }
}

// Scratch's stack pointers are the two `i32`s at its header, `hdr`.
class $Scratch extends $Space {
  constructor(exports) {
    super(exports.memory, exports.table, false);
    this.hdr = exports.hdr.value >>> 2;
    this.i32_ = new Int32Array(0);
  }

  hp() {
    if (this.i32_.length === 0) this.i32_ = new Int32Array(this.u8().buffer);
    return this.i32_;
  }

  get sp() {
    return this.hp()[this.hdr];
  }

  set sp(v) {
    this.hp()[this.hdr] = v;
  }

  get tsp() {
    return this.hp()[this.hdr + 1];
  }

  set tsp(v) {
    this.hp()[this.hdr + 1] = v;
  }

  alloc(size, align) {
    const p = (this.sp + align - 1) & -align;
    const end = p + size;
    const have = this.u8().length;
    if (end > have) this.memory.grow(Math.ceil((end - have) / 65536));
    this.sp = end;
    return p;
  }

  talloc(n) {
    const b = this.tsp;
    const end = b + n;
    if (end > this.table.length) this.table.grow(end - this.table.length);
    this.tsp = end;
    return b;
  }

  mark() {
    return [this.sp, this.tsp];
  }

  reset(m) {
    for (let i = m[1]; i < this.tsp; i++) this.table.set(i, null);
    this.sp = m[0];
    this.tsp = m[1];
  }
}

// The externref table's ranges, read from the globals its definer exports
// (IR.md §14). The constants give languages that can't produce `undefined`
// a slot to read it from.
function $table(table, stack, constant, heap) {
  if (!(0 < stack && stack <= constant && constant + 4 <= heap)) {
    throw new Error(`invalid externref table bases ${stack}, ${constant}, ${heap}`);
  }
  if (table.length < heap) table.grow(heap - table.length);
  table.set(constant, undefined);
  table.set(constant + 1, null);
  table.set(constant + 2, true);
  table.set(constant + 3, false);
  return { table, stack, constant, heap };
}

// `results = single` (IR.md §12): the spill is empty at every crossing.
function $spillCheck() {
  if (!$B.spill_empty()) $spillFault(2);
}

// A spill misuse, reported by the bridge module or the glue.
function $spillFault(kind) {
  $B.spill_reset();
  const what = ["a spilled result was read that wasn't pushed", "the spill overflowed", "spilled results were left unread"];
  throw new TypeError(`results = single: ${what[kind]}`);
}

// A language export under `results = single` or `throws = js`: drains the
// spilled slots after slot 0, or throws the error the language set with
// `("tw:err", "set:E")` in place of its results.
function $spilled(f, takes, single, pending) {
  return (...a) => {
    if (single) $spillCheck();
    if (pending) {
      $pending = null;
      $B.pending.value = 0;
    }
    let r;
    try {
      r = f(...a);
    } catch (e) {
      if (single) $B.spill_reset();
      throw e;
    }
    if (pending && $B.pending.value) {
      const e = $pending;
      $pending = null;
      $B.pending.value = 0;
      if (single) $B.spill_reset();
      throw e;
    }
    if (takes.length === 0) {
      if (single) $spillCheck();
      return r;
    }
    const out = [r];
    for (const t of takes) out.push(t());
    $spillCheck();
    return out;
  };
}

// An erased position holds boxes, host references and wrappers, never a
// language object, which enters one only through `tw:box` (IR.md §13). A
// Wasm GC object is a frozen object with no prototype.
function $erased(v) {
  if (typeof v === "object" && v !== null && Object.getPrototypeOf(v) === null && Object.isFrozen(v)) {
    throw new TypeError("a language object in an erased position: box it with `tw:box`");
  }
  return v;
}

// Pairs language handles with their JS wrappers (IR.md §14): one wrapper per
// live handle. Linear handles are numbers, held weakly through a WeakRef
// whose finalizer calls the language's drop export; reference handles key a
// WeakMap, and the shared GC needs no finalizer.
const $flagged = Symbol("tw.flagged");

class $Handles {
  constructor(what, linear, drop) {
    this.what = what;
    this.linear = linear;
    this.drop = drop;
    this.live = linear ? new Map() : new WeakMap();
    this.of = new WeakMap();
    this.dead = new WeakSet();
    this.once_ = new WeakSet();
    this.reg = linear
      ? new FinalizationRegistry((rec) => {
          if (this.live.get(rec.h) === rec) this.live.delete(rec.h);
          if (rec.drop) $drop(rec.drop, rec.h);
        })
      : null;
  }

  wrap(h, make, drop = this.drop) {
    const rec = this.live.get(h);
    if (this.linear) {
      const w = rec?.ref.deref();
      if (w !== undefined) return w;
      // A collected wrapper's drop is still pending: the new wrapper takes
      // it over, so the handle is dropped once.
      if (rec) this.reg.unregister(rec);
    } else if (rec !== undefined) {
      return rec;
    }
    const w = make();
    this.pair(w, h, drop);
    return w;
  }

  // The wrapper `tw:new` made for an instance of a class with an imported
  // base, which can't be recreated (§17).
  paired(h, what) {
    const rec = this.live.get(h);
    const w = this.linear ? rec?.ref.deref() : rec;
    if (w === undefined) {
      throw new TypeError(`a ${what} handle with no live wrapper: construct it with \`tw:new\` and keep the wrapper`);
    }
    return w;
  }

  pair(w, h, drop = this.drop) {
    this.of.set(w, h);
    if (this.linear) {
      const rec = { h, ref: new WeakRef(w), drop };
      this.live.set(h, rec);
      this.reg.register(w, rec, rec);
    } else {
      this.live.set(h, w);
    }
  }

  owns(w, C) {
    return this.of.has(w) && w instanceof C;
  }

  unwrap(w, C, what) {
    if (!this.of.has(w) || !(w instanceof C)) $fail(what ?? this.what, w);
    if (this.dead.has(w)) throw new TypeError(`a destroyed ${what ?? this.what}`);
    return this.of.get(w);
  }

  tryUnwrap(w, C) {
    return this.of.has(w) && w instanceof C && !this.dead.has(w) ? this.of.get(w) : null;
  }

  // `@once`: the first call revokes the wrapper (§14).
  once(w) {
    this.once_.add(w);
    this.flag(w);
  }

  // A function wrapper calls with the handle it closes over until it's
  // flagged `@once` or destroyed, when it takes the checked `call`.
  flag(w) {
    if (typeof w === "function") w[$flagged] = true;
  }

  call(w) {
    const h = this.unwrap(w, Function);
    if (this.once_.has(w)) this.dead.add(w);
    return h;
  }

  destroy(h) {
    const rec = this.live.get(h);
    if (rec === undefined) return;
    this.live.delete(h);
    if (this.linear) {
      this.reg.unregister(rec);
      const w = rec.ref.deref();
      if (w !== undefined) {
        this.dead.add(w);
        this.flag(w);
      }
    } else {
      this.dead.add(rec);
      this.flag(rec);
    }
  }
}

// The constructor `tw:new` names, read by the next exported constructor.
let $newArm;

function $fn(v, what) {
  if (typeof v !== "function") $fail(what, v);
  return v;
}

// A language error as its JS error class (§15).
function $construct(C, msg, props) {
  const e = C === globalThis.DOMException ? new C(msg, props.name) : new C(msg);
  for (const k in props) {
    if (e[k] !== props[k]) Object.defineProperty(e, k, { value: props[k], configurable: true, writable: true });
  }
  return e;
}

// The cleanup stack (IR.md §15): `kind, ptr, size, align` records of the
// allocations conversions make, memory (0), table slots (1), or the `size`
// strings of an `arr<str>` at `ptr` (2), below `n`.
// Each conversion marks `n`, and resets it once the language owns what it
// pushed. One shared stack saves growing an array per call, and nested
// conversions only ever touch the records above their own mark. `n` is kept
// apart because setting an array's `length` is slow.
const $cx = { a: [], n: 0 };

function $cxPush(cx, kind, p, size, align) {
  const a = cx.a;
  const n = cx.n;
  a[n] = kind;
  a[n + 1] = p;
  a[n + 2] = size;
  a[n + 3] = align;
  cx.n = n + 4;
}

// Releases a failed conversion's allocations: the records above `m`.
function $release(cx, m) {
  const a = cx.a;
  for (let i = cx.n - 4; i >= m; i -= 4) {
    try {
      if (a[i] === 0) $L.free(a[i + 1], a[i + 2], a[i + 3]);
      else if (a[i] === 1) $L.tfree(a[i + 1], a[i + 2]);
      else $freeStrs(a[i + 1], a[i + 2]);
    } catch {}
  }
  cx.n = m;
}

// The bridge's return area, the first bytes of its scratch memory, where a
// JS import writes its results past the first.
let $ra = null;
function $ret() {
  if ($ra === null || $ra.i32.length === 0) {
    const b = $B.memory.buffer;
    const o = $B.hdr.value + 8;
    $ra = {
      i32: new Int32Array(b, o, 16),
      i64: new BigInt64Array(b, o, 8),
      f32: new Float32Array(b, o, 16),
      f64: new Float64Array(b, o, 8),
    };
  }
  return $ra;
}

function $lowerUtf8(s, v, cx) {
  const n = v.length;
  if (n === 0) return [1, 0];
  if (s === $L) {
    let p = s.alloc(n, 1, null);
    let len = n;
    let i = 0;
    if (n < 24) {
      const u8 = s.u8();
      for (; i < n; i++) {
        const c = v.charCodeAt(i);
        if (c > 0x7f) break;
        u8[p + i] = c;
      }
    }
    if (i !== n) {
      len = $fillUtf8(v, p, n);
      p = $filled;
    }
    if (cx) $cxPush(cx, 0, p, len, 1);
    return [p, len];
  }
  const p = s.alloc(n * 3, 1);
  const len = $te.encodeInto(v, s.u8().subarray(p, p + n * 3)).written;
  s.sp = p + len;
  return [p, len];
}

// Where `$fillUtf8` left its buffer.
let $filled = 0;

// Encodes `v` into the language's `n`-byte buffer at `p`, `n` being its
// length: exact for ASCII, and grown and shrunk past it. Returns the byte
// length, leaving the buffer's address in `$filled`. Frees the buffer if
// it fails.
function $fillUtf8(v, p, n) {
  let cap = n;
  let i = 0;
  let len;
  if (n < 24) {
    const u8 = $L.u8();
    for (; i < n; i++) {
      const c = v.charCodeAt(i);
      if (c > 0x7f) break;
      u8[p + i] = c;
    }
    len = i;
  } else {
    // An ASCII string fills the buffer exactly, and encodeInto stops
    // before a character that doesn't fit.
    const r = $te.encodeInto(v, $L.u8().subarray(p, p + cap));
    i = r.read;
    len = r.written;
  }
  if (i !== n) {
    const rest = v.slice(i);
    try {
      const grown = len + rest.length * 3;
      p = $L.realloc(p, cap, 1, grown);
      cap = grown;
      len += $te.encodeInto(rest, $L.u8().subarray(p + len, p + cap)).written;
      if (len !== cap) {
        p = $L.realloc(p, cap, 1, len);
        cap = len;
      }
    } catch (e) {
      try {
        $L.free(p, cap, 1);
      } catch {}
      throw e;
    }
  }
  $filled = p;
  return len;
}

// Frees the first `n` strings of the `(ptr, len)` descriptors at `p`.
function $freeStrs(p, n) {
  for (let i = 0; i < n; i++) {
    const dv = $L.dv();
    const len = dv.getUint32(p + i * 8 + 4, true);
    if (len !== 0) {
      try {
        $L.free(dv.getUint32(p + i * 8, true), len, 1);
      } catch {}
    }
  }
}

// `arr<str>` elements into the language's memory as `(ptr, len)` pairs at
// `p` (IR.md §11). A call from JS into Wasm costs more than a small
// allocation, so the bridge's `allocn` makes every element's buffer in one
// call, through `tw:realloc` in its `rtab` table.
function $lowerStrs(v, n, p, cx, check) {
  const strs = new Array(n);
  for (let i = 0; i < n; i++) strs[i] = check(v[i]);
  if ($B.rtab.get(0) === null) $B.rtab.set(0, $L.realloc_);
  const m = $S.mark();
  try {
    const q = $S.alloc(n * 8, 4);
    const hp = $S.hp();
    const sz = q >> 2;
    const at = sz + n;
    for (let i = 0; i < n; i++) {
      hp[sz + i] = strs[i].length;
      hp[at + i] = 0;
    }
    try {
      $B.allocn(q, n);
    } catch (e) {
      for (let i = 0; i < n; i++) {
        if (hp[at + i] > 1) {
          try {
            $L.free(hp[at + i] >>> 0, hp[sz + i], 1);
          } catch {}
        }
      }
      throw e;
    }
    let i = 0;
    try {
      for (; i < n; i++) {
        const v = strs[i];
        const k = v.length;
        let ptr = 1;
        let len = 0;
        if (k !== 0) {
          ptr = hp[at + i] >>> 0;
          len = k;
          let j = 0;
          if (k < 24) {
            const u8 = $L.u8();
            for (; j < k; j++) {
              const c = v.charCodeAt(j);
              if (c > 0x7f) break;
              u8[ptr + j] = c;
            }
          }
          if (j !== k) {
            len = $fillUtf8(v, ptr, k);
            ptr = $filled;
          }
        }
        const dv = $L.dv();
        dv.setInt32(p + i * 8, ptr, true);
        dv.setInt32(p + i * 8 + 4, len, true);
      }
    } catch (e) {
      // The filled buffers below the failed one, which freed itself, and
      // the unfilled ones past it.
      $freeStrs(p, i);
      for (let j = i + 1; j < n; j++) {
        if (hp[at + j] > 1) {
          try {
            $L.free(hp[at + j] >>> 0, hp[sz + j], 1);
          } catch {}
        }
      }
      throw e;
    }
    // One record releases every element, from the descriptors at `p`.
    if (cx) $cxPush(cx, 2, p, n, 0);
  } finally {
    $S.reset(m);
  }
}

// TextDecoder's fixed cost is about that of decoding 24 ASCII bytes by hand.
function $liftUtf8(s, p, len) {
  const u8 = s.u8();
  if (len < 24) {
    const units = new Array(len);
    for (let i = 0; i < len; i++) {
      const c = u8[p + i];
      if (c > 0x7f) return $td.decode(u8.subarray(p, p + len));
      units[i] = c;
    }
    return String.fromCharCode.apply(null, units);
  }
  return $td.decode(u8.subarray(p, p + len));
}

function $lowerWtf16(s, v, cx) {
  const n = v.length;
  if (n === 0) return [2, 0];
  const p = s.alloc(n * 2, 2, cx);
  const dv = s.dv();
  for (let i = 0; i < n; i++) dv.setUint16(p + i * 2, v.charCodeAt(i), true);
  return [p, n];
}

function $liftWtf16(s, p, len) {
  const dv = s.dv();
  let out = "";
  for (let i = 0; i < len; i += 4096) {
    const end = Math.min(len, i + 4096);
    const units = new Array(end - i);
    for (let j = i; j < end; j++) units[j - i] = dv.getUint16(p + j * 2, true);
    out += String.fromCharCode.apply(null, units);
  }
  return out;
}

// A fresh copy in both directions (IR.md §11).
// Reading `.buffer` of a small fresh array moves it off-heap, so both
// directions copy through views of the element type. `set` copies through
// a clone when its source shares the memory.
function $lowerVec(s, v, C, cx) {
  const size = C.BYTES_PER_ELEMENT;
  const n = v.length;
  if (n === 0) return [size, 0];
  const p = s.alloc(n * size, size, cx);
  new C(s.u8().buffer, p, n).set(v);
  return [p, n];
}

function $liftVec(s, p, len, C) {
  const n = len * C.BYTES_PER_ELEMENT;
  if (len === 0) return new C(0);
  if (p % C.BYTES_PER_ELEMENT === 0) return new C(s.u8().buffer, p, len).slice();
  return new C(s.u8().slice(p, p + n).buffer);
}

// `bigint` as two's-complement little-endian `u64` limbs: the fewest that
// keep its sign, at least one (IR.md §10). Any length reads back, and none
// is 0.
function $bigLimbs(v) {
  const limbs = [];
  for (;;) {
    const limb = BigInt.asUintN(64, v);
    limbs.push(limb);
    v >>= 64n;
    if (v === (limb >> 63n ? -1n : 0n)) return BigUint64Array.from(limbs);
  }
}

function $bigOf(limbs) {
  let v = 0n;
  for (let i = limbs.length; i-- > 0; ) v = (v << 64n) | limbs[i];
  return BigInt.asIntN(64 * limbs.length, v);
}

function $writeback(s, p, len, t) {
  s.u8().set(new Uint8Array(t.buffer, t.byteOffset, len * t.BYTES_PER_ELEMENT), p);
}

// A zero-copy view over the language's memory for `*T[]` (IR.md §11),
// created per call and bounds checked. An `unshared` view of a shared
// memory is a copy instead.
function $view(C, p, n, unshared) {
  const buf = $L.memory.buffer;
  if (!(p >= 0 && n >= 0 && p + n * C.BYTES_PER_ELEMENT <= buf.byteLength)) {
    throw new TypeError("a view outside the memory");
  }
  const v = new C(buf, p, n);
  return unshared && $L.shared ? v.slice() : v;
}

// A future the language holds (IR.md §16): a promise, settled state for
// synchronous polls, and the waker to call when it settles. `check`
// validates a resolution, which otherwise rejects with its TypeError.
class $Future {
  constructor(promise, check, cancel) {
    this.promise = promise;
    this.state = 0;
    this.value = undefined;
    this.waker = -1;
    this.cancel = cancel;
    this.dropped = false;
    this.taken = false;
    promise.then(
      (v) => {
        if (this.dropped) return;
        try {
          if (check) check(v);
          this.settle(1, v);
        } catch (e) {
          this.settle(2, e);
        }
      },
      (e) => {
        if (!this.dropped) this.settle(2, e);
      },
    );
  }

  settle(state, value) {
    this.state = state;
    this.value = value;
    this.cancel = null;
    const w = this.waker;
    this.waker = -1;
    if (w !== -1) $wake(w);
  }
}

function $future(v, check, cancel) {
  return v instanceof $Future ? v : new $Future(Promise.resolve(v), check, cancel ?? null);
}

// A synchronous throw folded into a rejected future (§16).
function $rejected(e) {
  return new $Future(Promise.reject(e), null, null);
}

function $promise(v) {
  return v instanceof $Future ? v.promise : v;
}

function $isFuture(f) {
  if (!(f instanceof $Future)) throw new TypeError(`expected a future, got ${$describe(f)}`);
  if (f.dropped) throw new TypeError("a dropped future");
  return f;
}

function $poll(f, waker) {
  $isFuture(f);
  if (f.state === 0) f.waker = waker;
  return f.state;
}

function $settled(f, state) {
  $isFuture(f);
  if (f.state !== state || f.taken) {
    throw new TypeError(state === 1 ? "taking a future that isn't resolved" : "the error of a future that isn't rejected");
  }
  f.taken = true;
  const v = f.value;
  f.value = undefined;
  return v;
}

// Dropping cancels: the cancel function runs once, its failure swallowed,
// and a later settlement is ignored.
function $dropFuture(f) {
  if (!(f instanceof $Future) || f.dropped) return;
  f.dropped = true;
  f.waker = -1;
  f.value = undefined;
  const c = f.state === 0 ? f.cancel : null;
  f.cancel = null;
  if (c) {
    try {
      c();
    } catch {}
  }
}

function $newFuture() {
  let resolve, reject;
  const p = new Promise((a, b) => {
    resolve = a;
    reject = b;
  });
  return [new $Future(p, null, null), { resolve, reject }];
}

function $resolver(r) {
  if (r === null || typeof r !== "object" || typeof r.resolve !== "function") $fail("a resolver", r);
  return r;
}

// Drives the language's loops (§16): one coalesced macrotask per loop,
// then a timer for its next sleep. `live` says whether the language can
// still be entered.
class $Loops {
  constructor(tick, sleep, live) {
    this.tick_ = tick;
    this.sleep = sleep;
    this.live = live;
    this.loops = new Map();
    this.queue = [];
    this.ch = null;
  }

  state(loop) {
    let s = this.loops.get(loop);
    if (!s) this.loops.set(loop, (s = { posted: false, timer: null }));
    return s;
  }

  available(loop) {
    this.post(loop);
  }

  post(loop) {
    const s = this.state(loop);
    if (s.timer !== null) {
      clearTimeout(s.timer);
      s.timer = null;
    }
    if (s.posted) return;
    s.posted = true;
    if (!this.ch) {
      this.ch = new MessageChannel();
      this.ch.port1.onmessage = () => this.run();
    }
    this.queue.push(loop);
    this.ch.port1.ref?.();
    this.ch.port2.postMessage(0);
  }

  run() {
    const loop = this.queue.shift();
    if (this.queue.length === 0) this.ch.port1.unref?.();
    const s = this.loops.get(loop);
    if (!s || !s.posted) return;
    s.posted = false;
    this.tick(loop);
  }

  tick(loop) {
    if (!this.live()) return;
    if (this.tick_(loop, 64)) return this.post(loop);
    const n = this.sleep(loop);
    if (n === $NICHE53) return;
    const ns = $liftU53(n);
    if (ns === 0) return this.post(loop);
    const s = this.state(loop);
    s.timer = setTimeout(() => {
      s.timer = null;
      this.tick(loop);
    }, Math.ceil(ns / 1e6));
  }

  drop(loop) {
    const s = this.loops.get(loop);
    if (!s) return;
    if (s.timer !== null) clearTimeout(s.timer);
    this.loops.delete(loop);
  }
}
