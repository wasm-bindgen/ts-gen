// Benchmark helpers: `case_(name, ops, run)` times `run(k)`, which performs
// `k` operations, and reports the best ns/op over several rounds.
import fs from "node:fs";

const PROFILE = process.env.BENCH_PROFILE;
const NAME = process.env.BENCH_NAME;
const OUT = process.env.BENCH_OUT;
// `BENCH_CASE` runs only the cases whose names contain it.
const ONLY = process.env.BENCH_CASE;

export const profile = PROFILE;

export function case_(name, ops, run, { rounds = 7, setup } = {}) {
  if (ONLY && !name.includes(ONLY)) return NaN;
  setup?.();
  run(Math.max(1, Math.floor(ops / 10)));
  run(Math.max(1, Math.floor(ops / 10)));
  globalThis.gc?.();
  let best = Infinity;
  let heap = 0;
  for (let r = 0; r < rounds; r++) {
    setup?.();
    const h0 = process.memoryUsage().heapUsed;
    const t0 = process.hrtime.bigint();
    run(ops);
    const t = Number(process.hrtime.bigint() - t0) / ops;
    heap = Math.max(heap, (process.memoryUsage().heapUsed - h0) / ops);
    if (t < best) best = t;
  }
  return report(name, best, heap);
}

// `case_` for a `run(k)` returning a promise.
export async function caseAsync(name, ops, run, { rounds = 7 } = {}) {
  if (ONLY && !name.includes(ONLY)) return NaN;
  await run(Math.max(1, Math.floor(ops / 10)));
  globalThis.gc?.();
  let best = Infinity;
  let heap = 0;
  for (let r = 0; r < rounds; r++) {
    const h0 = process.memoryUsage().heapUsed;
    const t0 = process.hrtime.bigint();
    await run(ops);
    const t = Number(process.hrtime.bigint() - t0) / ops;
    heap = Math.max(heap, (process.memoryUsage().heapUsed - h0) / ops);
    if (t < best) best = t;
  }
  return report(name, best, heap);
}

function report(name, best, heap) {
  const row = { bench: NAME, profile: PROFILE, case: name, ns: +best.toFixed(1), heapB: Math.round(heap) };
  console.log(`${PROFILE.padEnd(12)} ${name.padEnd(36)} ${best.toFixed(1).padStart(12)} ns/op  ~${row.heapB} B/op heap`);
  if (OUT) fs.appendFileSync(OUT, JSON.stringify(row) + "\n");
  return best;
}

export function compile(f, builtins = false) {
  const bytes = fs.readFileSync(f);
  return new WebAssembly.Module(bytes, builtins ? { builtins: ["js-string"] } : {});
}
