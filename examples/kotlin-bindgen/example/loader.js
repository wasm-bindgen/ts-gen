// Instantiates the linked Kotlin module through the bridge and publishes
// its exports: the default export, the Durable Object and the RPC
// entrypoint. Kotlin's import object supplies its own js_code snippets,
// the exception tag and the `'` string constants; the bridge supplies
// every tw import.
import { importObject } from './worker.import-object.mjs';
import { instantiate } from './bridge.js';
import language from './worker.wasm';
import bridge from './bridge.wasm';

instantiate(language, bridge, importObject).exports._start();

export { default, Counter, Greeter } from './bridge.js';
