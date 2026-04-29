# Conventions: TypeScript → Rust

This document is the canonical reference for how `ts-gen` translates
TypeScript declarations into `wasm-bindgen` Rust bindings. It covers the
patterns we handle, what they emit, and the rules behind each translation.

Conventions are listed roughly from simplest to most complex. New
conventions belong here first; tests and codegen come second. Keep the file
in sync with the snapshot fixtures (`tests/fixtures/*.d.ts` paired with
`tests/snapshots/*.rs`).

> **Maintenance**: when changing a convention or adding a new one, update
> this file in the same PR. Diff-only snapshot changes that aren't
> documented are a smell.

---

## 1. Primitive types

| TypeScript                      | Rust                       |
| ------------------------------- | -------------------------- |
| `string`                        | `String` / `&str`          |
| `number`                        | `f64`                      |
| `boolean`                       | `bool`                     |
| `bigint`                        | `i64`                      |
| `void`                          | `()` (or omitted from sig) |
| `undefined`                     | `Undefined`                |
| `null`                          | `()`                       |
| `any` / `unknown`               | `JsValue`                  |
| `never`                         | `JsValue`                  |
| `object`                        | `Object`                   |

`String` vs `&str` (or `Object` vs `&Object` etc.) is chosen by argument
position vs return position — see [§ Argument vs Return Position].

## 2. Optional and nullable types

* `T | null` → `Option<T>` in return position, `Option<T>` in argument
  position. (`null`-only is rare; treated like `undefined`.)
* `T | undefined` and `T | null | undefined` → also `Option<T>`. We coalesce
  at parse time; the rendered union has no separate `null`/`undefined`
  arm.
* `T?` on a property → `Option<T>`. The setter takes `Option<T>` too, so
  callers can clear the property by passing `None`.
* `f(x?: T)` (optional parameter) → produces an overload pair, *not* an
  `Option<T>` parameter. See [§ Optional Parameter Truncation].

## 3. Property accessors

```ts
interface Foo {
  readonly bar: string;
  baz: number;
}
```

emits:

```rust
#[wasm_bindgen(method, getter)]
pub fn bar(this: &Foo) -> String;

#[wasm_bindgen(method, getter)]
pub fn baz(this: &Foo) -> f64;
#[wasm_bindgen(method, setter, js_name = "baz")]
pub fn set_baz(this: &Foo, val: f64);
```

`readonly` properties get a getter only. Non-readonly properties get both;
the setter is named `set_<snake_case>`.

## 4. Naming conversion

* JS `camelCase` / `PascalCase` identifiers → Rust `snake_case` for fns,
  `PascalCase` for types.
* `js_name = "..."` is emitted whenever the Rust ident differs from the JS
  ident, so `wasm-bindgen` binds to the correct runtime name.
* Reserved Rust keywords (e.g. `type`, `match`, `move`) are emitted as raw
  identifiers (`r#type`).

## 5. JS-name collisions with `js_sys` glob imports

The generated preamble does `use js_sys::*;`, which brings every `js_sys`
type into scope. A locally declared class with a colliding name (e.g.
`WebAssembly.Global` vs `js_sys::Global`) would be ambiguous at every
reference. We resolve this by:

1. Picking a suffixed Rust ident (`Global` → `Global_`) for the
   internal declaration.
2. Keeping `js_name = "Global"` on the wasm-bindgen attr so the JS-side
   binding is unaffected.
3. Re-exporting under the original name so the public Rust path is
   unchanged: `pub use Global_ as Global;`

```rust
pub mod web_assembly {
    use js_sys::*;
    #[wasm_bindgen]
    extern "C" {
        #[wasm_bindgen(extends = Object, js_name = "Global", js_namespace = "WebAssembly")]
        pub type Global_;
        #[wasm_bindgen(constructor, catch, js_name = "Global")]
        pub fn new(...) -> Result<Global_, JsValue>;
    }
    pub use Global_ as Global;   // public face
}
```

Consumers always write `web_assembly::Global`. The `_` suffix is an
internal detail.

## 6. Classes

```ts
class Greeter {
  constructor(name: string);
  greet(): string;
}
```

emits a `pub type Greeter;` plus method bindings inside an
`extern "C"` block. Constructors get `#[wasm_bindgen(constructor, catch)]`
because JS constructors can always throw.

`abstract` classes skip the constructor (you can't `new` an abstract
class).

## 7. Interfaces (class-like vs dictionary)

Interfaces are classified by shape (see `parse/classify.rs`):

* **Class-like** — has methods, used as a type: emit `pub type Foo;` plus
  member bindings, just like a class. No constructor.
* **Dictionary** — properties only, no methods, used as an options bag:
  emit `pub type Foo;` plus a Rust-side `new()` factory and (usually) a
  fluent builder. Setters/getters are still emitted as wasm-bindgen
  bindings, the builder just calls them. See [§ 8 Dictionary builders].

Multiple interface declarations with the same name + module context merge:
their members union, their `extends` lists merge.

## 8. Dictionary builders

Dictionaries get an `impl` block with a Rust-side `new()` plus an
ergonomic builder, depending on the property mix.

### 8a. All-mutable properties → builder

```ts
interface ResponseInit {
  status?: number;
  statusText?: string;
  headers?: Headers;
}
```

emits:

```rust
impl ResponseInit {
    pub fn new() -> Self { /* unchecked_into of new Object */ }
    pub fn builder() -> ResponseInitBuilder { /* … */ }
}

pub struct ResponseInitBuilder { inner: ResponseInit }
impl ResponseInitBuilder {
    pub fn status(mut self, val: f64) -> Self {
        self.inner.set_status(val);
        self
    }
    pub fn status_text(mut self, val: &str) -> Self { /* … */ }
    pub fn headers(mut self, val: &Headers) -> Self { /* … */ }
    pub fn build(self) -> ResponseInit { self.inner }
}
```

The builder's setter methods take ownership of `self` and return `Self`,
producing the standard fluent chain:

```rust
let init = ResponseInit::builder().status(200.0).status_text("OK").build();
```

### 8b. Required properties → fallible `build() -> Result<T, JsValue>`

When at least one property is required (no `?` and no `readonly` exempting
it), the builder tracks unset required props with a `required: u64`
bitmask and `build()` returns a `Result`:

```rust
pub struct NumberIndexedBuilder {
    inner: NumberIndexed,
    required: u64,
}
impl NumberIndexedBuilder {
    pub fn length(mut self, val: f64) -> Self {
        self.inner.set_length(val);
        self.required &= 18446744073709551614u64; // clear bit 0
        self
    }
    pub fn build(self) -> Result<NumberIndexed, JsValue> {
        if self.required != 0 {
            let mut missing = Vec::new();
            if self.required & 1u64 != 0 {
                missing.push("missing required property `length`");
            }
            return Err(JsValue::from_str(&format!(
                "{}: {}",
                stringify!(NumberIndexed),
                missing.join(", ")
            )));
        }
        Ok(self.inner)
    }
}
```

The bitmask supports up to 64 required properties per dictionary, which
is more than enough for any realistic options bag.

### 8c. Has any `readonly` property → `new()` only, no builder

A dictionary that exposes a `readonly` property can't be fully
constructed from the JS side via plain setter calls (the runtime would
reject the write). To avoid silently producing invalid objects, ts-gen
falls back to emitting only `new()`:

```rust
impl FooWithReadonly {
    pub fn new() -> Self { /* unchecked_into of new Object */ }
}
```

Callers must construct the underlying JS object themselves and cast
into `FooWithReadonly` — there's no Rust-side builder for these.

### 8d. Variadic / overloaded setters

When a property's setter has union types (or the spec defines multiple
setter overloads), each variant becomes a distinct builder method with
the standard `_with_<type>` suffix — same naming machinery as method
overloads (see [§ 11 Signature flattening]). Calling more than one of
them on the same builder overwrites earlier values.

```ts
interface ResponseInit {
  headers?: Headers | string[][] | Record<string, string>;
}
```

emits builder methods `headers`, `headers_with_array`,
`headers_with_record`.

## 9. `var X: { new(...): T }` patterns

The TypeScript trick of declaring a class via a variable + interface pair:

```ts
interface MyClass {
  foo(): void;
}
declare var MyClass: {
  new (n: number): MyClass;
};
```

is recognised at parse time. The variable contributes the constructor,
the interface contributes the methods, and the merged result emits as a
single class. See `merge.rs` for the heuristic.

## 10. Module-scoped constructor variables

```ts
declare module "cloudflare:email" {
  let _EmailMessage: {
    prototype: EmailMessage;
    new (from: string, to: string, raw: ReadableStream | string): EmailMessage;
  };
  export { _EmailMessage as EmailMessage };
}
```

Recognised as a module-scoped class declaration. Output:

```rust
pub mod email {
    #[wasm_bindgen(module = "cloudflare:email")]
    extern "C" {
        #[wasm_bindgen]
        pub type EmailMessage;
        #[wasm_bindgen(constructor, catch)]
        pub fn new(from: &str, to: &str, raw: &str) -> Result<EmailMessage, JsValue>;
        // ...
    }
}
```

The `export { _EmailMessage as EmailMessage }` rename is captured in the
`TypeRegistry::export_renames` map and applied to the public name.

## 11. Signature flattening

TypeScript can describe a single callable in several ways that all mean
"there are multiple shapes of arguments this accepts": explicit
overloads, optional parameters, union-typed parameters, variadics. They
go through one shared pipeline in
`codegen::signatures::expand_signatures` so the binding names and
dedup behaviour stay consistent across the four cases.

### 10a. The four input forms

```ts
// Explicit overloads — one or more sibling declarations sharing a name.
function fetch(url: string): Promise<Response>;
function fetch(url: string, init: RequestInit): Promise<Response>;

// Optional parameters — `?` produces a truncation variant per prefix.
function f(a: string, b?: number, c?: boolean): void;

// Union-typed parameters — expand via cartesian product.
function send(body: string | ArrayBuffer): void;

// Variadic — `...args` becomes a wasm-bindgen `variadic` slice.
function log(...args: any[]): void;
```

Conceptually all four describe the same thing: a JS callable whose
caller has more than one valid argument shape.

### 10b. The pipeline

For every JS callable, `ts-gen`:

1. **Per-overload expansion**: For each overload's parameter list,
   generate every concrete variant. Optional params produce truncation
   variants (one per prefix `[(a), (a, b), (a, b, c)]`); union params
   expand via cartesian product (`(string | ArrayBuffer)` →
   `[(string), (ArrayBuffer)]`); a trailing variadic stays trailing.
2. **Cross-overload dedup**: When multiple overloads expand to the same
   concrete parameter list, drop the duplicates. Two overloads that
   both truncate to `(callback)` produce only one binding.
3. **Suffix assignment**: Across all surviving expansions, compute
   `_with_X` / `_with_X_and_Y` suffixes that disambiguate them. The
   shortest-arity (or first) variant gets `""`; longer variants are
   named after their additional parameters.

The output is a `Vec<{ name_suffix, params }>` — a focused
parameter-axis result. The per-callable layer (`build_signatures`)
then handles the orthogonal decisions (base name, async-ness, `try_`
companions, doc, error type).

### 10c. Examples

Optional truncation:

```rust
pub fn f(a: &str);
pub fn f_with_b(a: &str, b: f64);
pub fn f_with_b_and_c(a: &str, b: f64, c: bool);
```

Union-typed parameters:

```rust
pub fn send(body: &str);
pub fn send_with_array_buffer(body: &ArrayBuffer);
```

Variadic — when it's the only differentiator from a sibling overload,
the parameter name becomes the suffix:

```rust
#[wasm_bindgen(variadic)]
pub fn log(args: &[JsValue]);
```

Mixed inputs — overload + optional + union:

```ts
function show(): void;
function show(value: string | number, opts?: ShowOpts): void;
```

Phase 1 expands overload 1 over `string | number × optional opts` to
four variants: `(string)`, `(number)`, `(string, opts)`,
`(number, opts)`. Phase 2 dedups against overload 0's empty `()`.
Phase 3 assigns suffixes:

```rust
pub fn show();
pub fn show_with_value(value: &str);
pub fn show_with_value_and_opts(value: &str, opts: &ShowOpts);
pub fn show_with_value_a(value: f64);
pub fn show_with_value_a_and_opts(value: f64, opts: &ShowOpts);
```

`compute_rust_names` in `codegen::signatures` handles the suffix
disambiguation, including readability adjustments when the same
parameter name appears in multiple alternatives.

### 10d. Why a single pipeline

Treating optional, union, overload, and variadic as one parameter-axis
problem keeps suffix naming consistent (the `_with_X` rules apply to
every binding regardless of which input form produced it),
keeps cross-overload dedup honest (truncation collisions get dropped
once across all input forms), and keeps the per-callable layer
oblivious to the combinatorics.

An earlier design interleaved the four expansions across the codebase
and produced near-duplicate bindings whenever two of them combined.

## 12. Methods + the `try_<name>` companion

For sync methods and free functions, every primary binding gets a fallible
companion that catches synchronous JS exceptions:

```rust
#[wasm_bindgen(method)]
pub fn frobnicate(this: &Foo) -> String;

#[wasm_bindgen(method, catch, js_name = "frobnicate")]
pub fn try_frobnicate(this: &Foo) -> Result<String, JsValue>;
```

The non-`try_` form panics on JS throw; the `try_` form returns `Result`.
Setters and constructors don't get a `try_` companion (setters never
catch; constructors always catch).

## 13. `Promise<T>` returns become `async fn`

```ts
function fetch(url: string): Promise<Response>;
```

emits a single async signature with `catch`:

```rust
#[wasm_bindgen(catch)]
pub async fn fetch(url: &str) -> Result<Response, JsValue>;
```

* The async + catch form is already fallible — no `try_fetch` companion.
* `wasm-bindgen` rewraps the `T` as `Promise<T>` on the JS side.
* Constructors and setters never become async.

## 14. `@throws` JSDoc → typed error

```ts
/**
 * @throws {ImagesError} if upload fails
 */
upload(file: File): Promise<ImageMetadata>;
```

emits `Result<ImageMetadata, ImagesError>` instead of `Result<_,
JsValue>`. Recognised forms:

* `@throws {TypeError} when foo` — single type
* `@throws {TypeError | RangeError} when bar` — inline union
* `@throws {@link ImagesError} if foo` — `{@link X}` collapses to `X`
* Multiple `@throws` lines aggregate into one effective union
* `@throws Sentence describing condition.` — pure prose, no structured
  type extracted

The original prose surfaces in the rendered doc as an `## Errors` section.

## 15. Subtyping LUB across unions

`TypeRef::Union` resolution applies a Least Upper Bound across its members
based on the subtyping lattice in `codegen::subtyping`:

```text
TypeError                            -> Error
TypeError | RangeError               -> Error      (both subclass Error)
TypeError | string                   -> JsValue    (no shared ancestor below Object)
BadRequestError | NotFoundError      -> StreamError (when both extend StreamError)
```

The lattice is built from:

* A static `BUILTIN_PARENTS` table for JS Error / DOM / collection /
  typed-array hierarchies.
* User-declared `class extends X` / `interface extends X` chains, walked
  through the codegen scope.

When the deepest common ancestor is `Object` (no useful narrowing), the
union erases to `JsValue` — the existing default. This rule is universal:
it applies to `@throws` unions and to any TS union return type.

## 16. Module declarations and namespace nesting

```ts
declare module "cloudflare:email" {
  // ...
}
```

emits a `pub mod email { ... }` (the prefix `cloudflare:` is stripped to
the part after the last `:`). All bindings inside use
`#[wasm_bindgen(module = "cloudflare:email")]`. Cross-module references
go through the parent scope's re-exports.

```ts
namespace WebAssembly {
  class Module { ... }
}
```

emits a `pub mod web_assembly { ... }` with `#[wasm_bindgen(js_namespace
= "WebAssembly")]` on each member. The namespace lookup is one-deep —
nested namespaces are not yet supported.

## 17. Type aliases and `export { X as Y }`

* `type Foo = Bar;` → `pub type Foo = Bar;` if `Bar` is a recognised
  type, or chases the alias chain to its terminal during codegen.
* `export { Local as Public };` (sourceless) → recorded in
  `TypeRegistry::export_renames`. The local declaration is published
  under the public name, and any redundant alias stub is suppressed.
* `export { X as Y } from "...";` (with source) → registered as an import
  from the named module.

## 18. String and numeric enums

```ts
enum Color { Red = "red", Green = "green" }
```

emits a `pub enum Color { Red, Green }` plus serde-aware `to_string` /
`try_from_str` impls. `wasm-bindgen` doesn't handle string enums
natively, so we lower these to Rust-side enums + a `JsValue` round-trip.

Numeric enums lower similarly with explicit discriminant values.

## 19. Multiple-context name resolution

When the same name appears in different `ModuleContext`s (e.g. a global
`interface EmailMessage` and a `cloudflare:email`-scoped class
`EmailMessage`), they remain distinct types. `merge_class_pairs` keys on
`(name, ModuleContext)` to keep them separate. Same-context same-name
still merges as expected.
