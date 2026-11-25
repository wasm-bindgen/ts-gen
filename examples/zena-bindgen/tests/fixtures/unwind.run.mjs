// Runs tests/fixtures/unwind.tw's exports, printing how each ends. An
// export still pending after its wait prints `pending`.
import { readFile } from 'node:fs/promises';

const wasm = async (file, options) =>
  new WebAssembly.Module(await readFile(new URL(file, import.meta.url)), options);

globalThis.boom = () => {
  throw new SyntaxError('boom!');
};
globalThis.typed = () => {
  throw new TypeError('typed!');
};
globalThis.later = () => new Promise((resolve) => setTimeout(resolve, 1));

// What the loop throws, where no promise carries it.
process.on('uncaughtException', () => {});

const bridge = await import('./bridge.js');
bridge.instantiate(
  await wasm('./language.wasm', { builtins: ['js-string'], importedStringConstants: '_' }),
  await wasm('./bridge.wasm'),
  {
    env: { captureStackTrace: () => null, formatStackTrace: () => null },
    _: new Proxy({}, { get: (_, text) => text }),
  },
);

const pending = new Promise((resolve) => setTimeout(resolve, 200, 'pending'));
for (const name of process.argv.slice(2)) {
  let end;
  try {
    end = await Promise.race([bridge[name](), pending]);
  } catch (e) {
    end = String(e);
  }
  console.log(`${name}: ${end}`);
}
process.exit(0);
