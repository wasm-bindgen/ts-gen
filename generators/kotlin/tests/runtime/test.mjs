// Calls the probe's Kotlin exports through the bridge: a spilled tuple
// result, an error set through set:ProbeError, a future that rejects
// through reject<ProbeError>, `undefined` read as none, the box helpers,
// and a box's none, ref.null, written as each node's default. Prints one
// line per call.
import { readFileSync } from 'node:fs';
const seen = [];
globalThis.ext = { none: () => undefined, seen: (v) => seen.push(v), seenNull: (v) => seen.push(v) };
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
lines.push(`spill empty: ${spillEmpty()}`);
process.stdout.write(lines.join('\n') + '\n');
