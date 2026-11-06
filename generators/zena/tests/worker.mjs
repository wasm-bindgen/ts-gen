// node zena-worker.mjs <merged.wasm> <builtins.js>: runs the Zena worker
// example the way workerd does, and checks its replies.
import { readFile } from 'node:fs/promises';
import { pathToFileURL } from 'node:url';
import assert from 'node:assert/strict';

const [wasmPath, builtinsPath] = process.argv.slice(2);
const builtins = await import(pathToFileURL(builtinsPath));
const module = await WebAssembly.compile(await readFile(wasmPath), {
  builtins: ['js-string'],
  importedStringConstants: 'wasm:js/string-constants',
});
const env = { captureStackTrace: () => null, formatStackTrace: () => null };
const { exports } = await WebAssembly.instantiate(module, { './builtins.js': builtins, env });
const worker = exports.default.value;
const bindings = { GREETING: 'Hello from node' };

const get = await worker.fetch(new Request('http://example.com/a?b=c'), bindings, {});
assert.equal(
  await get.text(),
  'GET http://example.com/a?b=c\nx-zena: (none)\nGREETING: Hello from node\nbody: \n',
);

const post = await worker.fetch(
  new Request('http://example.com/', { method: 'POST', headers: { 'x-zena': 'hi' }, body: 'Hello!' }),
  bindings,
  {},
);
assert.equal(
  await post.text(),
  'POST http://example.com/\nx-zena: hi\nGREETING: Hello from node\nbody: Hello!\n',
);
console.log('zena worker ok');
