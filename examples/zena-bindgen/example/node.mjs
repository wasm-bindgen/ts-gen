// Runs the routes node can serve. The wasm modules load as
// WebAssembly.Module, as workerd's do, the language with the js-string
// builtins and `_` string constants. The routes over workerd's own APIs
// (HTMLRewriter, the cache, Durable Objects, WebSocketPair, RPC, queues)
// are left to `./run.sh serve`.
import { register } from 'node:module';

register('data:text/javascript,' + encodeURIComponent(`
// The base classes the worker's exported classes extend.
const workers = 'export class DurableObject {} export class WorkerEntrypoint {}';
export async function resolve(specifier, ctx, next) {
  if (specifier === 'cloudflare:workers') {
    return { url: 'data:text/javascript,' + encodeURIComponent(workers), shortCircuit: true };
  }
  return next(specifier, ctx);
}
export async function load(url, ctx, next) {
  if (url.endsWith('.wasm')) {
    const opts = url.endsWith('/worker.wasm')
      ? '{ builtins: ["js-string"], importedStringConstants: "_" }' : '{}';
    return { format: 'module', shortCircuit: true, source:
      'export default new WebAssembly.Module(await (await import("node:fs/promises")).readFile(new URL(' + JSON.stringify(url) + ')), ' + opts + ');' };
  }
  return next(url, ctx);
}`));

// Workers' scheduler, which node lacks, and its numeric timer ids.
globalThis.scheduler ??= { wait: (ms) => new Promise((r) => setTimeout(r, ms)) };
const { setTimeout: timeout } = globalThis;
globalThis.setTimeout = (...args) => Number(timeout(...args));

const { default: worker } = await import(new URL('./loader.js', import.meta.url));
const { default: helper } = await import(new URL('./helper.js', import.meta.url));

// The helper service stands behind the HELPER binding and the global
// fetch, as config.capnp wires it, and calls back into the worker.
const waiting = [];
const ctx = { waitUntil: (p) => waiting.push(p), passThroughOnException() {} };
const env = { GREETING: 'Hello from node' };
const main = {
  fetch: (url) => worker.fetch(new Request(url), env, ctx),
  scheduled: async (controller) => {
    await worker.scheduled(controller, env, ctx);
    return { outcome: 'ok' };
  },
};
// As a fetch does, it rejects when the request's signal aborts.
env.HELPER = { fetch: async (input, init) => {
  const request = new Request(input, init);
  const aborted = new Promise((_, reject) => {
    request.signal.addEventListener('abort', () => reject(request.signal.reason));
  });
  return Promise.race([helper.fetch(request, { MAIN: main }), aborted]);
} };
globalThis.fetch = env.HELPER.fetch;

async function show(label, path, init) {
  const res = await worker.fetch(new Request(`http://127.0.0.1${path}`, init), env, ctx);
  process.stdout.write(`== ${label}\n${await res.text()}[${res.status}]\n\n`);
  await Promise.all(waiting.splice(0));
}

const form = new FormData();
form.set('name', 'zena');
form.set('upload', new File(['hello from a file\n'], 'upload.txt', { type: 'text/plain' }));

await show('GET /', '/');
await show('POST /echo', '/echo', {
  method: 'POST',
  headers: { 'user-agent': 'tour', 'x-tour': '1', 'x-zena': 'hi' },
  body: 'Hello!',
});
await show('GET /url?b=2&a=1', '/url?b=2&a=1');
await show('POST /form (a field and a file)', '/form', { method: 'POST', body: form });
await show('POST /json', '/json', {
  method: 'POST',
  body: '{"item":"widget","quantity":2,"tags":["a","b"]}',
});
await show('GET /crypto', '/crypto');
await show('GET /stream', '/stream');
await show('GET /timers', '/timers');
await show('GET /fetch', '/fetch');
await show('GET /fail', '/fail');
await show('GET /scheduled', '/scheduled');
await show('GET /nowhere', '/nowhere');
// The helper's slow route is still waiting.
process.exit(0);
