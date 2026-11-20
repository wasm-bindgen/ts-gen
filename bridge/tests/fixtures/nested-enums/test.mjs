import { assert, lang } from "./harness.mjs";

class Zoned {}
class Day {}
let next;
Object.assign(globalThis, { Zoned, Day, relativeTo: () => next });

const { instantiate } = await import("./bridge.js");
const fs = await import("node:fs");
const compile = (f) => new WebAssembly.Module(fs.readFileSync(f));
const L = lang(instantiate(compile("lang.wasm"), compile("bridge.wasm")));
const pick = (v) => {
  next = v;
  return L.call("relativeTo()");
};
const head = (v) => pick(v).slice(0, 3);

// Every nested arm is tested in one tier order, so the class `Day` is
// tested before the dictionary `ZonedFields`: `[outer, inner, ...slots]`.
const day = new Day();
assert.deepEqual(head(day), [2, 1, day]);
const fields = { zone: "UTC" };
assert.deepEqual(head(fields), [1, 2, fields]);
// Within a tier, arm order: `ZonedLike.Zoned` before `DayLike.Zoned`.
const zoned = new Zoned();
assert.deepEqual(head(zoned), [1, 1, zoned]);
L.balanced(() => {
  const [outer, inner, ref, p, len] = pick("2020-01-01");
  assert.deepEqual([outer, inner, ref], [1, 3, null]);
  assert.equal(L.takeStr(p, len), "2020-01-01");
});
L.throwsType(() => pick(5), /ZonedLikeOrDayLike/);

console.log("ok");
