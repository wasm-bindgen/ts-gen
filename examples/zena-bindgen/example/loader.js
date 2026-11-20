// Instantiates the Zena module through the bridge and publishes its
// exports as the Worker. Beside the bridge's `tw` imports the module
// takes Zena's own for the JS target: string literals as `_` imports
// named by their text, which a host that compiles with
// importedStringConstants supplies itself, the stack trace hooks of its
// Error, and JS's number formatting and string methods.
import { instantiate } from './bridge.js';
import language from './worker.wasm';
import bridge from './bridge.wasm';

const zena = {
  env: { captureStackTrace: () => null, formatStackTrace: () => null },
  _: new Proxy({}, { get: (_, text) => text }),
  'zena:host:number': {
    String,
    toPrecision: (value, digits) => value.toPrecision(digits),
    parseFloat,
  },
  'zena:host:string': {
    toLowerCase: (s) => s.toLowerCase(),
    toUpperCase: (s) => s.toUpperCase(),
    indexOf: (s, needle, from) => s.indexOf(needle, from),
    replaceAll: (s, from, to) => s.replaceAll(from, to),
  },
};
instantiate(language, bridge, zena);

export * from './bridge.js';
export { default } from './bridge.js';
