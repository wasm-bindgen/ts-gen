# ts-gen-bridge

The `tw` bridge generator. Given a ts-gen IR, a profile and a compiled
language module, it writes the bridge that connects the module to JS:

* `bridge.js`: ESM glue that converts between JS values and the module's
  wire forms, validates arguments, dispatches overloads, maps errors, and
  pairs language handles with JS wrappers. It also exports the IR's exports
  as JS classes, functions and namespaces.
* `bridge.wasm`: a small Wasm module providing every import whose wire holds
  a GC reference, since JS can't read GC objects. It translates them to and
  from linear forms in its own scratch memory and table. When nothing needs
  it, it holds only that scratch space.

[IR.md](../IR.md) is the normative spec for the IR, the profiles, the wire
layouts and the `tw` protocol. This crate implements §11–§19 of it.

## CLI

```text
ts-gen-bridge link <in.wasm> <links.tsv>... -o <out.wasm>
ts-gen-bridge generate <file.ir> <language.wasm> -o <dir> [--profile <profile>] [--with <lib.ir>]... [--wat]
```

`generate` reads the language module as `.wasm` or `.wat`, and writes
`bridge.js` and `bridge.wasm` to `<dir>` (plus `bridge.wat` with `--wat`).
`<profile>` is a file holding the profile text, or the text itself
(`all-host, results = single, throws = js`). It defaults to `all-linear`.
The settings are IR.md §12's; `strict-null = on`, for example, makes a
`@js(null) T?` reject `undefined` from JS.
`--with` supplies the lib IRs the IR's `with` names.

`link` is for languages that can't choose an import's `(module, name)`.
Kotlin/Wasm, for example, imports every `@JsFun` from `js_code` under the
function's qualified name. Their generator lists each such import with the
key it stands for in `links.tsv`, and `link` renames the imports to match.
Only the names change.

A Rust or Zena module generates directly:

```sh
ts-gen --input worker.d.ts --ir worker.ir
# ... compile the language module against the IR's bindings ...
ts-gen-bridge generate worker.ir worker.wasm --profile all-gc -o out
```

A Kotlin module links first:

```sh
ts-gen-bridge link worker.wasm bindings/links.tsv -o out/worker.wasm
ts-gen-bridge generate worker.ir out/worker.wasm \
  --profile "all-host, results = single, throws = js" -o out
```

## Using the bridge

```js
import { instantiate, Counter, util } from "./out/bridge.js";

const instance = instantiate(
  await WebAssembly.compileStreaming(fetch("worker.wasm")),
  await WebAssembly.compileStreaming(fetch("bridge.wasm")),
  { /* the language module's other imports */ },
);
```

`instantiate(language, bridge, imports)` instantiates the bridge module and
then the language module, and returns the language instance. A glue module
holds one instance. The IR's exports are the glue module's named exports.
They're usable once `instantiate` has returned.

Two diagnostics are exported for tests:

* `scratchUse()` returns the bridge scratch stack pointers, which are back at
  their base between calls.
* `spillEmpty()` is true when the `results = single` spill is empty.

## Runtime requirements

The glue targets the current Wasm platform: WasmGC, exception handling with
`exnref` (or the legacy instructions under `eh = legacy`), JS string
builtins (`wasm:js-string`), memory64 for 64-bit profiles, and
`FinalizationRegistry`. Node 26 has all of them, as does workerd.

## Testing

```sh
just test                              # everything, including the bridge
cargo test -p ts-gen-bridge            # the bridge only
CI=1 cargo test -p ts-gen-bridge       # fail, rather than skip, without Node 26
BLESS=1 cargo test -p ts-gen-bridge    # re-bless the snapshots
```

Each directory in `tests/fixtures/` holds the following:

* `ir.ts` (an IR);
* an optional `profile` (default `all-linear`);
* an optional `lib.ts`;
* the language module, as either:
  * a `lang.wat` written by hand over `tests/runtime.wat`, or
  * a generated pass-through module, which imports each member, exports a
    `call:<key>` forwarder for it, and adds the trampolines and exports
    that `protocol` lists;
* a `test.mjs`.

The generated `bridge.js` and `bridge.wat` are snapshotted in
`tests/snapshots/`. With Node 26 or later on `PATH` (or under `~/.nvm`), each
fixture's `test.mjs` runs against the built modules, with `harness.mjs` for
memory and leak checks.
