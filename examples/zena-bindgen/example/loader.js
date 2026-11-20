// Instantiates the Zena module through the bridge and publishes the
// default export as the Worker. Zena's string literals are `_` imports
// named by their text, which a host that compiles with
// importedStringConstants supplies itself, and Zena's Error imports its
// stack trace hooks from `env`.
import { instantiate } from './bridge.js';
import language from './worker.wasm';
import bridge from './bridge.wasm';

const env = { captureStackTrace: () => null, formatStackTrace: () => null };
const strings = new Proxy({}, { get: (_, text) => text });
instantiate(language, bridge, { env, _: strings });

export { default, Counter } from './bridge.js';
