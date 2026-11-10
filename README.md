# ts-gen

Generate bindings from TypeScript `.d.ts` declaration files: Rust for
[`wasm-bindgen`](https://github.com/wasm-bindgen/wasm-bindgen), and, through
the ts-gen IR and the `tw` bridge, Kotlin/Wasm and Zena.

## Overview

`ts-gen` reads TypeScript `.d.ts` files. It writes Rust source with
`#[wasm_bindgen]` extern blocks directly ([Rust](#rust-wasm-bindgen)), or the
**ts-gen IR**, a language-neutral description of a JS API that any Wasm
language can bind against.

```text
.d.ts -ts-gen-------> Rust (#[wasm_bindgen])
.d.ts -ts-gen --ir--> app.ir -ts-gen-abi lower-> JSON -> Kotlin / Zena bindings -> app.wasm
                      app.ir + app.wasm -ts-gen-bridge-> bridge.js + bridge.wasm
```

## The ts-gen IR

[IR.md](IR.md) is the spec. An IR file declares classes, interfaces, enums,
callbacks, generics, async and exceptions in a small text format. Each
member is keyed by its canonical signature (`Request#text()`), which is also
its Wasm import name. A **profile** fixes how values cross the boundary:

* linear memory, WasmGC or host carriers;
* the string encoding;
* the exception and result conventions.

The [`ts-gen-abi`](abi) crate holds the IR's parser, printer and validator,
the profile and layout rules, and the `tw` protocol names. It lowers an IR
to per-member wire layouts for generators that don't link it:

```sh
ts-gen --input worker.d.ts --provides ExportedHandler.fetch --ir worker.ir
ts-gen-abi lower worker.ir --profile "all-host, results = single, throws = js" > worker.json
```

`--ir` takes the producer options: `--provides`, `--once`, `--no-cancel`,
`--bound`, `--with`, `--emit-lib`, `--root` and `--strict-nullable`. See
`ts-gen --help` for each.

## The tw bridge

[`ts-gen-bridge`](bridge) generates the JS side for a compiled language
module, its IR and its profile. `bridge.js` is the ESM glue: it converts and
validates values, dispatches overloads and maps errors, and exports the
IR's exports as JS classes and functions. `bridge.wasm` translates GC values
that JS can't read. See [bridge/README.md](bridge/README.md).

## Languages

| language | profile | generator | status |
|---|---|---|---|
| Rust | wasm-bindgen | `ts-gen` (this crate) | the CLI below; every type read from the IR |
| Kotlin/Wasm | `all-host, results = single, throws = js` | [generators/kotlin](generators/kotlin) | imports, exports and async, through the bridge |
| Zena | `all-gc` with host strings | [generators/zena](generators/zena) | imports, monomorphized generics |

**Kotlin**: `ts-gen-kotlin <lowered.json> <out-dir> [--package <pkg>]
[--runtime]` writes Kotlin externals from `ts-gen-abi lower`'s JSON. It also
writes a `links.tsv` for `ts-gen-bridge link`, since Kotlin names its own
imports. Build the generator with `./gradlew installDist` in
`generators/kotlin` (JDK 21). [examples/kotlin-worker](examples/kotlin-worker)
runs a Worker end to end.

**Zena**: `ts-gen-zena <input.ir> [-o <out.zena>]` writes `@external`
declarations. They need the Zena compiler fork with the `tw` option on
`@external`.

## Development

```sh
just test          # unit, snapshot and bridge runtime tests (Node 26)
just test-kotlin   # the Kotlin generator: snapshots, Kotlin/Wasm compiles, runtime (JDK 21, Node 26)
just test-zena     # the Zena generator; compiles with ZENA=<zena-cli> ZENA_REPO_ROOT=<checkout>
just test-integration  # wasm-bindgen integration (wasm32 target, wasm-bindgen-cli)
just fmt-check && just clippy
```

`just test` runs the Kotlin and Zena tests too when their tools are present,
and skips them otherwise. Under `CI` a missing tool is a failure, unless
`TSGEN_SKIP_KOTLIN` or `TSGEN_SKIP_ZENA` is set. The main CI job sets both,
and separate jobs run each toolchain. `BLESS=1` re-blesses snapshots.

## Rust (wasm-bindgen)

### Pipeline

```text
.d.ts files
  -> oxc_parser (AST)
  -> First Pass phase 1: collect all type names into TypeRegistry
  -> First Pass phase 2: populate full IR (resolve references, merge var+interface patterns)
  -> Assembly: group by ModuleContext, resolve inheritance chains, classify types
  -> Code Generation: IR -> syn::File -> prettyplease -> .rs files
```

### Installation

#### CLI

```sh
cargo install ts-gen
```

#### Library

```sh
cargo add ts-gen --no-default-features
```

The `cli` feature (enabled by default) pulls in `clap` and `walkdir` for the
binary. Disable it if you only need the library API.

### Usage

#### CLI

```sh
ts-gen --input types.d.ts --output src/bindings.rs
```

Process a directory of declaration files:

```sh
ts-gen --input ./typings/ --output src/bindings.rs --lib-name my-js-lib
```

Map external types from other crates:

```sh
ts-gen --input types.d.ts --output src/bindings.rs \
  --external "Blob=::web_sys::Blob" \
  --external "node:buffer=node_buffer_sys" \
  --external "Intl.DateTimeFormatOptions=::my_intl::Options"
```

Route untyped JS exceptions into your own error type (must implement
`From<JsValue>`):

```sh
ts-gen --input types.d.ts --output src/bindings.rs --error-type crate::Error
```

Keep `@throws {A | B}` unions as a generated `JsException` enum instead of
collapsing them to their common ancestor:

```sh
ts-gen --input types.d.ts --output src/bindings.rs --error-enum
```

Select the output surface. `--input` defines the type universe; `--export`
picks what is emitted: a file (its global declarations), a JS module
specifier (its declarations, in the module's `pub mod`), or a declaration name
(that export of any file or module, plus the types its bindings reference):

```sh
ts-gen --input node_modules/@cloudflare/workers-types/index.d.ts --output src/bindings.rs \
  --export Request --export Response --export KVNamespace --export cloudflare:workers
```

Layer `@tw-abi` / `@throws` tags onto a large declaration file from a side
file (see `CONVENTIONS.md`, "`--annotate` side files"):

```sh
ts-gen --input node_modules/@cloudflare/workers-types/index.d.ts \
  --output src/bindings.rs \
  --annotate types/annotations.d.ts
```

Opt generic imports into wasm-bindgen's experimental per-monomorphization
codegen:

```sh
ts-gen --input types.d.ts --output src/bindings.rs --experimental-generic-mono
```

Emit one `Result` form per sync method and function instead of a trapping
primary plus a `try_<name>` companion:

```sh
ts-gen --input types.d.ts --output src/bindings.rs --fallible
```

Keep `T | null | undefined` three-state. By default a return, awaited value or
getter admitting `null`, `undefined` or both reads as `Option<T>`, and a
`T | null | undefined` write passes `undefined`. With the flag the producer
keeps it as the IR's `T??`, so it reads as `Option<Nullable<T>>` and its
writes gain a `_with_null` variant. `T | null` and `T | undefined` are the
same either way:

```sh
ts-gen --input types.d.ts --output src/bindings.rs --strict-nullable
```

#### Library API

```rust
use ts_gen::{parse_source, codegen};

let source = r#"
  export declare class Greeter {
    constructor(name: string);
    greet(): string;
  }
"#;

let (module, gctx) = parse_source(source, Some("my-lib"))?;
let rust_code = codegen::generate(&module, &gctx)?;
println!("{rust_code}");
```

Options are set through `GenerateOptions` builder methods (its fields are
private):

```rust
let options = codegen::GenerateOptions::new()
    .error_type("crate::Error")
    .experimental_generic_mono(true)
    .strict_nullable(false);
let rust_code = codegen::generate_with_options(&module, &gctx, &options)?;
```

### Supported TypeScript constructs

- Classes (including abstract classes and inheritance)
- Interfaces (classified as class-like or dictionary types)
- Functions and variables
- String and numeric enums
- Type aliases
- Namespaces
- Module declarations
- Generics (`JsGeneric` erasure by default, with experimental
  per-monomorphization support)

### Known limitations

- **External generic type arguments** are not preserved unless the target is a
  recognized `js_sys` container; locally declared generic types keep theirs.
- The parsed IR uses `Rc<str>` internally and is `!Send` / `!Sync`.

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT) at your option.
