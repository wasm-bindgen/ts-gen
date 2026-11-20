// Runs the routes node can serve. The wasm modules load as
// WebAssembly.Module, as workerd's do; `cloudflare:workers` is a stub of
// its WorkerEntrypoint; `scheduler` and the GREETER binding are shims.
import { registerHooks } from 'node:module';

const workers = 'export class WorkerEntrypoint { constructor(ctx, env) { this.ctx = ctx; this.env = env; } }';
registerHooks({
  resolve(specifier, context, next) {
    if (specifier === 'cloudflare:workers') {
      return { url: 'data:text/javascript,' + encodeURIComponent(workers), shortCircuit: true };
    }
    return next(specifier, context);
  },
  load(url, context, next) {
    if (url.endsWith('.wasm')) {
      return { format: 'module', shortCircuit: true, source:
        `export default new WebAssembly.Module((await import("node:fs")).readFileSync(new URL(${JSON.stringify(url)})));` };
    }
    return next(url, context);
  },
});

// Workers' scheduler, which node lacks, and its setTimeout, which returns
// a number where node's returns an object.
const setTimeoutObject = globalThis.setTimeout;
globalThis.setTimeout = (...args) => Number(setTimeoutObject(...args));
globalThis.scheduler ??= { wait: (ms) => new Promise((resolve) => setTimeoutObject(resolve, ms)) };

const { default: worker, Greeter } = await import(new URL('./loader.js', import.meta.url));
const tasks = [];
const ctx = { waitUntil: (p) => { tasks.push(p); }, passThroughOnException() {} };
const env = { GREETING: 'Hello from node' };
// A service binding to the entrypoint is an RPC stub: every call is async.
const greeter = new Greeter(ctx, env);
env.GREETER = new Proxy({}, { get: (_, name) => async (...args) => greeter[name](...args) });

const form = new FormData();
form.set('title', 'Kotlin');
form.set('upload', new File(['file contents'], 'note.txt', { type: 'text/plain' }));
const tour = [
  ['GET /', '/'],
  ['POST /echo', '/echo', { method: 'POST', headers: { 'x-kotlin': 'hi' }, body: 'Hello!' }],
  ['GET /url?tag=a&tag=b&q=kotlin', '/url?tag=a&tag=b&q=kotlin'],
  ['POST /form (a string and a file)', '/form', { method: 'POST', body: form }],
  ['POST /json', '/json', { method: 'POST', body: '{"item":"tea","quantity":2,"tags":["hot","green"]}' }],
  ['POST /json (not JSON)', '/json', { method: 'POST', body: '{nope' }],
  ['GET /crypto', '/crypto'],
  ['GET /stream', '/stream'],
  ['GET /timers', '/timers'],
  ['GET /timers (again)', '/timers'],
  ['GET /rpc', '/rpc'],
  ['GET /fail', '/fail'],
  ['GET /fail/plain', '/fail/plain'],
];
for (const [label, path, init] of tour) {
  let reply;
  try {
    const res = await worker.fetch(new Request(`http://127.0.0.1${path}`, init), env, ctx);
    reply = await res.text();
  } catch (e) {
    reply = `rejected: ${e.name}: ${e.message}\n`;
  }
  await Promise.all(tasks.splice(0));
  process.stdout.write(`\n== ${label}\n${reply}${reply.endsWith('\n') ? '' : '\n'}`);
}
