# ts-gen

Generate [`wasm-bindgen`](https://github.com/wasm-bindgen/wasm-bindgen) Rust bindings from TypeScript `.d.ts` declaration files.

## Overview

`ts-gen` reads TypeScript `.d.ts` files and produces Rust source code containing `#[wasm_bindgen]` extern blocks, ready to compile against `wasm-bindgen`.

### Pipeline

```text
.d.ts files
  -> oxc_parser (AST)
  -> First Pass phase 1: collect all type names into TypeRegistry
  -> First Pass phase 2: populate full IR (resolve references, merge var+interface patterns)
  -> Assembly: group by ModuleContext, resolve inheritance chains, classify types
  -> Code Generation: IR -> syn::File -> prettyplease -> .rs files
```

## Installation

### CLI

```sh
cargo install ts-gen
```

### Library

```sh
cargo add ts-gen --no-default-features
```

The `cli` feature (enabled by default) pulls in `clap` and `walkdir` for the
binary. Disable it if you only need the library API.

## Usage

### CLI

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

Flags:

| Flag | Description |
|------|-------------|
| `-i`, `--input` | `.d.ts` files or directories to read |
| `-o`, `--output` | The output file |
| `--target` | What to generate (see below) |
| `-E`, `--export` | Files, module specifiers and declaration names the output covers (default: every input file) |
| `-e`, `--external` | External type mappings, `Foo=::path::Foo` or `node:*=node_sys::*` |
| `--lib-name` | JS module specifier, required when the `.d.ts` has top-level exports |
| `--no-web-sys` | Don't map web platform types to `web_sys` by default |
| `--error-type` | Rust error type for bindings without `@throws`, replacing `JsValue` |
| `--error-enum` | One enum (default `JsException`) over every `@throws` type |
| `--errors-as-error` | Shorthand for `--error-type ::js_sys::Error` |
| `--fallible` | One `Result` form per sync callable instead of a `try_<name>` companion |
| `--experimental-generic-mono` | Per-monomorphization codegen for generic imports |
| `--provides`, `--provides-app`, `--provides-bindings` | See below |
| `-v`, `--verbose` | Print the parsed IR |

### Targets (`--target`)

| Value  | Output |
|--------|--------|
| `rust` | wasm-bindgen extern blocks (the default) |
| `wat`  | The Wasm ABI as an annotated WAT import template ([ABI.md](ABI.md)) |

### Wasm ABI template (`--target wat`)

Generate the Wasm ABI for languages that compile directly to Wasm:

```sh
ts-gen --input types.d.ts --output types.abi.wat --target wat
```

The template lists every ABI function as an import, from `./bindings.wasm`
for a global and from its specifier for a `declare module` declaration,
and, with `--provides`, a stub export per provided function. `@abi.*`
annotations, which WAT tools ignore, carry the rest of the model: each
import's owner and JS operation, what each reference position names and
whether it may be `null` or `undefined`, and the owners' heritage and
docs. [ABI.md](ABI.md) specifies the format.

Write a module against the template by hand, or have a bindings generator
read it, then use `embed` to generate the runtime artifacts. A generator
needs nothing from ts-gen but the file.

### Provided functions (`--provides`)

The reverse direction: what the module provides to JS, as a `.d.ts` export
surface or an interface shorthand such as `ExportedHandler.fetch`. Writes
`provides.rs` (wasm-bindgen exports delegating to `super::app`) and an
`app.rs` scaffold next to the output; see "Provided functions" in
[CONVENTIONS.md](CONVENTIONS.md).

```sh
ts-gen --input worker.d.ts --output src/bindings.rs --provides ExportedHandler.fetch
```

| Flag | Description |
|------|-------------|
| `--provides` | A `.d.ts` export surface, or `Name[.member,...]` |
| `--provides-app` | Module path the wrapper delegates to; its last segment names the scaffold (default `super::app`). `--target rust` only |
| `--provides-bindings` | Module path of the bindings (default `super::<output stem>`, with `-` and `.` replaced by `_`). `--target rust` only |

### Bindings generators

Language bindings are written by separate programs that read the template:

* [`generators/zena`](generators/zena/README.md): Zena, a Rust crate that
  depends only on [`ts-gen-abi`](abi/), the model and its reader.
  `examples/zena-worker/` is a worker built with it.
* [`examples/kotlin-worker`](examples/kotlin-worker/README.md): Kotlin/Wasm,
  with a generator written in Kotlin and its own reader.

```sh
ts-gen --input worker.d.ts --output worker.abi.wat --target wat --provides ExportedHandler.fetch
ts-gen-zena worker.abi.wat -o bindings.zena
zena build main.zena -t js --imported-string-constants wasm:js/string-constants -o main.wasm
ts-gen embed --abi worker.abi.wat -m main.wasm -o out/
```

### Embedding (`ts-gen embed`)

Given a module written against the template, produce the bridging modules:

```sh
ts-gen embed --abi types.abi.wat -m hello.wasm -o out/
```

`--abi` reads the ABI from the template the module was written against.
`embed` can instead derive it again from the same `.d.ts` and flags, with
`-i types.d.ts --provides ExportedHandler.fetch`.

This creates `out/hello.wasm` (merged), `out/builtins.js`, and optionally
`out/bindings.wat` / `out/wrapper.wat` with `--text`. The merged module
imports only `builtins.js`, the js-string-builtins string constants and
any `declare module` specifiers the `.d.ts` declares.

Flags:

| Flag | Description |
|------|-------------|
| `--abi` | The template the module was written against, in place of `-i` |
| `-i`, `--input` | The `.d.ts` files the module was written against |
| `-m`, `--module` | The compiled language module |
| `--bindings-module` | Module the language imports global ABI functions from (default `./bindings.wasm`). Its imports that are not ABI functions are left to the host |
| `--imported-string-constants` | Namespace the bindings import string constants from (default `wasm:js/string-constants`): the one the merged module is compiled with |
| `-o`, `--out-dir` | Output directory |
| `--text` | Also write `.wat` text for debugging |
| `--wasm-merge` | Path to binaryen's `wasm-merge` (required; defaults to `$WASM_MERGE`, then `PATH`) |
| `--module-specifier` | Specifier `wrapper.wasm` imports the module under (default `./<module file name>`) |
| `--provides` | What the module provides, as for the main command |
| `--lib-name` | JS module specifier, required when the `.d.ts` has top-level exports |
| `-E`, `--export` | What the template covers, as for the main command; pass the same values |
| `-e`, `--external` | External mappings, as for the main command; pass the same values |

Hello-world flow:

```sh
# 1. Generate the template
ts-gen --input worker.d.ts --output worker.abi.wat --target wat --provides ExportedHandler.fetch

# 2. Write your module against it (e.g. hello.wat)
# 3. Assemble it
wasm-tools parse hello.wat -o hello.wasm

# 4. Embed
ts-gen embed --abi worker.abi.wat -m hello.wasm -o out/

# 5. Run (e.g. with workerd)
workerd serve config.capnp
```

### Library API

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
    .experimental_generic_mono(true);
let rust_code = codegen::generate_with_options(&module, &gctx, &options)?;
```

## Supported TypeScript constructs

- Classes (including abstract classes and inheritance)
- Interfaces (classified as class-like or dictionary types)
- Functions and variables
- String and numeric enums
- Type aliases
- Namespaces
- Module declarations
- Generics (`JsGeneric` erasure by default, with experimental
  per-monomorphization support)

## Known limitations

- **External generic type arguments** are not preserved unless the target is a
  recognized `js_sys` container; locally declared generic types keep theirs.
- The parsed IR uses `Rc<str>` internally and is `!Send` / `!Sync`.

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT) at your option.
