// Runs the worker in node: the wasm modules load as WebAssembly.Module,
// as workerd's do, the language with the js-string builtins and `_`
// string constants. Then one request goes to its fetch.
import http from 'node:http';
import { register } from 'node:module';

register('data:text/javascript,' + encodeURIComponent(`
export async function load(url, ctx, next) {
  if (url.endsWith('.wasm')) {
    const opts = url.endsWith('/worker.wasm')
      ? '{ builtins: ["js-string"], importedStringConstants: "_" }' : '{}';
    return { format: 'module', shortCircuit: true, source:
      'export default new WebAssembly.Module(await (await import("node:fs/promises")).readFile(new URL(' + JSON.stringify(url) + ')), ' + opts + ');' };
  }
  return next(url, ctx);
}`));

// Workers' scheduler, which node lacks.
globalThis.scheduler ??= { wait: (ms) => new Promise((r) => setTimeout(r, ms)) };

const { default: worker, Counter } = await import(new URL('./loader.js', import.meta.url));
// A server that never answers, whose connection closing shows the
// worker's fetch of it was cancelled.
let aborted = false;
const hang = http.createServer((req) => req.on('close', () => { aborted = true; }));
await new Promise((r) => hang.listen(0, '127.0.0.1', r));
const req = new Request('http://127.0.0.1/echo', {
  method: 'POST',
  headers: { 'x-zena': 'hi', 'x-hang': `http://127.0.0.1:${hang.address().port}/` },
  body: 'Hello!',
});
const env = { GREETING: 'Hello from node' };
const ctx = { waitUntil() {}, passThroughOnException() {} };
const res = await worker.fetch(req, env, ctx);
const text = await res.text();
await new Promise((r) => setTimeout(r, 50));
hang.closeAllConnections();
hang.close();
process.stdout.write(`${res.status} ${res.headers.get('content-type')}\n${text}aborted: ${aborted}\n`);

// The Durable Object, behind a COUNTER namespace standing in for
// workerd's: constructed once, then fetched twice through the worker.
const counter = new Counter({ id: 'counter' }, env);
env.COUNTER = { getByName: () => ({ fetch: (url) => counter.fetch(new Request(url)) }) };
for (let i = 0; i < 2; i++) {
  const r = await worker.fetch(new Request('http://127.0.0.1/count'), env, ctx);
  process.stdout.write(await r.text());
}

// Zena's errors in JS: a rejection with a TypeError arm, and a
// constructor's plain Error, which leaves as the fallback arm.
await worker.fetch(new Request('http://127.0.0.1/fail'), env, ctx).catch((e) => {
  process.stdout.write(`rejected: ${e.constructor.name}: ${e.message}\n`);
});
try {
  new Counter({ id: 'counter' }, { GREETING: '' });
} catch (e) {
  process.stdout.write(`constructor threw: ${e.constructor.name}: ${e.message}\n`);
}
