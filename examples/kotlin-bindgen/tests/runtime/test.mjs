// Calls the probe's Kotlin exports through the bridge: a spilled tuple
// result, an error set through set:ProbeError, a future that rejects
// through reject<ProbeError>, `undefined` read as none, the box helpers,
// a box's none, ref.null, written as each node's default, a suspend lambda
// as a callback's and a parameter's future, arrays copied each way, and a
// spread parameter as a vararg, and host type arguments kept typed.
// Prints one line per call.
import { readFileSync } from 'node:fs';
const seen = [];
globalThis.Shelf = class {
  constructor(...items) { this.items = items; }
  first() { return this.items[0]; }
};
globalThis.ext = {
  shelf: () => new Shelf('word'),
  stock: async (item) => new Shelf(item),
  size: (s) => s.items.length,
  none: () => undefined, seen: (v) => seen.push(v), seenNull: (v) => seen.push(v),
  later: async (f, x) => (await f(x)) + 1,
  settle: async (p) => (await p) + '!',
  bytes: (v) => v.map((b) => b + 1),
  words: (xs) => xs.map((x) => x.toUpperCase()),
  join: (sep, ...parts) => parts.join(sep),
  pick: (x) => x,
  soon: (f) => { setTimeout(f, 1); },
  listen: (l, x) => (typeof l === 'function' ? l(x) : l + x),
  attempt: async (f) => await f(),
};
import { importObject } from './probe.import-object.mjs';
import { instantiate, probe, spillEmpty, Counter } from './bridge.js';

const load = (f) => new WebAssembly.Module(readFileSync(new URL(f, import.meta.url)));
instantiate(load('./probe.wasm'), load('./bridge.wasm'), importObject).exports._start();

const show = (e) => `${e.constructor.name}: ${e.message}`;
const lines = [];
for (const [label, f] of [
  ['split(7)', () => probe.split(7)],
  ['split(8)', () => probe.split(8)],
  ['check(3)', () => probe.check(3)],
  ['check(0)', () => probe.check(0)],
  ['check(1)', () => probe.check(1)],
]) {
  try {
    lines.push(`${label} = ${JSON.stringify(f())}`);
  } catch (e) {
    lines.push(`${label} threw ${show(e)}`);
  }
}
for (const ok of [true, false]) {
  try {
    lines.push(`settle(${ok}) = ${await probe.settle(ok)}`);
  } catch (e) {
    lines.push(`settle(${ok}) rejected ${show(e)}`);
  }
}
lines.push(`absent: ${[undefined, null, "x"].map((v) => probe.absent(v)).join(",")}`);
lines.push(`imported: ${probe.imported()}`);
const c = probe.counter();
lines.push(`counter: ${c instanceof Counter} ${probe.count(c)}`);
lines.push(`modes: ${JSON.stringify([probe.mode(0), probe.mode(5)])} ${probe.modeOf("off")} ${probe.modeOf(9)}`);
const add = probe.adder();
lines.push(`adder: ${typeof add} ${add(41)} ${probe.callBoxed(add, 1)}`);
probe.writeNones();
lines.push(`box nones: ${String(probe.none())} ${seen.map(String).join(",")}`);
lines.push(`async callback: ${await probe.asyncCallback(7)}`);
lines.push(`made: ${await probe.made()}`);
lines.push(`bytes: ${probe.bytes()}`);
lines.push(`words: ${probe.words()}`);
lines.push(`join: ${probe.join()}`);
lines.push(`callback: ${await probe.soon()}`);
lines.push(`listen: ${probe.listen()}`);
lines.push(`attempt: ${await probe.attempt()}`);
lines.push(`generic: ${await probe.generic()}`);
lines.push(`spill empty: ${spillEmpty()}`);
process.stdout.write(lines.join('\n') + '\n');
