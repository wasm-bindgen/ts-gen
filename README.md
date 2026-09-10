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
  --external "node:buffer=node_buffer_sys"
```

Opt generic imports into wasm-bindgen's experimental per-monomorphization
codegen:

```sh
ts-gen --input types.d.ts --output src/bindings.rs --experimental-generic-mono
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
