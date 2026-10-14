// Instantiates the merged module and publishes its `default` export as
// the Worker. `env` holds the stack trace hooks Zena's `Error` imports.
import * as builtins from './builtins.js';
import mod from './main.wasm';
const env = { captureStackTrace: () => null, formatStackTrace: () => null };
const { exports } = new WebAssembly.Instance(mod, { './builtins.js': builtins, env });
export default exports.default.value;
