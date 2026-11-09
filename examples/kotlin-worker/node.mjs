// Runs the worker in node: the wasm modules load as WebAssembly.Module,
// as workerd's wasm modules do, then one request goes to its fetch.
import { register } from 'node:module';

register('data:text/javascript,' + encodeURIComponent(`
export async function load(url, ctx, next) {
  if (url.endsWith('.wasm')) {
    return { format: 'module', shortCircuit: true, source:
      'export default new WebAssembly.Module(await (await import("node:fs/promises")).readFile(new URL(' + JSON.stringify(url) + ')));' };
  }
  return next(url, ctx);
}`));

const { default: worker } = await import(new URL('./loader.js', import.meta.url));
const req = new Request('http://127.0.0.1/echo', { method: 'POST', headers: { 'x-kotlin': 'hi' }, body: 'Hello!' });
const res = await worker.fetch(req, { GREETING: 'Hello from node' }, { waitUntil() {}, passThroughOnException() {} });
process.stdout.write(`${res.status} ${res.headers.get('content-type')}\n${await res.text()}`);
