// Runs one embedded fixture: node embed.mjs <fixture> <merged.wasm> <builtins.js>
//
// Instantiates the merged module the way a host would (js-string builtins,
// imported string constants, `builtins.js`, and the modules the fixture
// `.d.ts` declares, whose exports are passed as values, as ESM integration
// does), then exercises the published handlers and checks what reached JS.

import { readFile } from 'node:fs/promises';
import { pathToFileURL } from 'node:url';
import assert from 'node:assert/strict';

const [fixture, wasmPath, builtinsPath] = process.argv.slice(2);

const logged = [];
globalThis.console = { log: (...args) => logged.push(args) };
globalThis.joinAll = (sep, ...parts) => parts.join(sep);
globalThis.describeFlags = (flags, strict) =>
  `${typeof flags.verbose}:${flags.verbose}:${typeof strict}:${strict}`;

const connected = [];
class Socket {
  static PROTOCOL = 'tls';
  startTls() { return this; }
  get closed() { return Promise.resolve(); }
}
const modules = {
  'cloudflare:sockets': {
    connect: (address) => { connected.push(address); return new Socket(); },
    Socket,
  },
  'embed:values': { count: 42, enabled: true, disabled: false, big: 7n },
};

const builtins = await import(pathToFileURL(builtinsPath));
const module = await WebAssembly.compile(await readFile(wasmPath), {
  builtins: ['js-string'],
  importedStringConstants: 'wasm:js/string-constants',
});
const imports = { './builtins.js': builtins };
for (const { module: m, name } of WebAssembly.Module.imports(module)) {
  if (Object.hasOwn(modules, m)) {
    (imports[m] ??= {})[name] = modules[m][name];
  } else if (m !== './builtins.js') {
    throw new Error(`unexpected import ${m} ${name}`);
  }
}
const { exports } = await WebAssembly.instantiate(module, imports);
const published = Object.fromEntries(
  Object.entries(exports).map(([k, v]) => [k, v instanceof WebAssembly.Global ? v.value : v]),
);

const fetch = (req, env = {}, ctx = {}) => published.default.fetch(req, env, ctx);
const get = new Request('http://example.com/path');
const post = (body, headers) => new Request('http://example.com/', { method: 'POST', body, headers });

switch (fixture) {
  case 'hello_sync': {
    const res = fetch(get);
    assert.ok(res instanceof Response);
    assert.equal(res.status, 200);
    assert.equal(res.headers.get('content-type'), 'text/plain');
    assert.equal(await res.text(), 'Hello from Wasm!');
    break;
  }
  case 'hello_jspi':
  case 'hello_anyref': {
    assert.equal(await (await fetch(get)).text(), 'Hello from Wasm!');
    const res = await fetch(post('bob', { 'x-name': 'Al' }));
    assert.equal(res.status, 200);
    assert.equal(await res.text(), 'Al: bob');
    break;
  }
  case 'hello_mixed': {
    assert.equal(await (await fetch(get)).text(), 'http://example.com/path');
    assert.equal(await (await fetch(post('echo'))).text(), 'echo');
    break;
  }
  case 'booleans': {
    assert.equal(await fetch(get).text(), 'boolean:true:boolean:false');
    break;
  }
  case 'variadic': {
    assert.equal(fetch(get), 'Hello, World');
    assert.deepEqual(logged, [['Hello', 'World']]);
    break;
  }
  case 'module_imports': {
    assert.equal(await fetch(get).text(), 'tls: ');
    assert.deepEqual(connected, ['example.com:443']);
    break;
  }
  case 'module_values': {
    assert.equal(published.count(), 42);
    assert.equal(published.enabled(), 1);
    assert.equal(published.disabled(), 0);
    assert.equal(published.big(), 7n);
    break;
  }
  case 'sys': {
    assert.equal(fetch(get).status, 204);
    assert.equal(fetch(post('', { 'x-status': '299' })).status, 300);
    break;
  }
  case 'nonnull': {
    assert.equal(await fetch(get).text(), 'http://example.com/path');
    assert.equal(await fetch(post('', { 'x-name': 'Al' })).text(), 'Al');
    // `Request.url` is `string`, imported as `(ref extern)`: a JS null traps.
    assert.throws(
      () => fetch({ url: null, headers: new Headers() }),
      (e) => e instanceof WebAssembly.RuntimeError,
    );
    break;
  }
  case 'exports_nested': {
    assert.equal(published.version(), 3);
    assert.equal(published.api.math.add(2, 3), 5);
    assert.equal(published.api.describe(get), 'http://example.com/path');
    assert.equal(await fetch(get).text(), 'ok');
    assert.deepEqual(Object.keys(exports).sort(), ['api', 'default', 'version']);
    break;
  }
  case 'anyref_ctor': {
    const res = fetch(get);
    assert.equal(res.headers.get('content-type'), 'text/plain');
    assert.equal(await res.text(), 'ok');
    break;
  }
  default:
    throw new Error(`no checks for fixture ${fixture}`);
}
