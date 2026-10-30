// Instantiates the merged module and publishes its `default` export as
// the Worker. Kotlin's generated import object supplies its own `js_code`
// snippets (the stdlib's; the ABI's were linked by `ts-gen embed`), the
// exception tag, and the `'` string constants for hosts that do not
// compile them in; `./builtins.js` comes from `ts-gen embed`.
import { importObject } from './worker.import-object.mjs';
import * as builtins from './builtins.js';
import mod from './worker.wasm';
const { exports } = new WebAssembly.Instance(mod, { ...importObject, './builtins.js': builtins });
export default exports.default.value;
