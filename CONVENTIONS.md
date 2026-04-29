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

## Contents

* [Primitive types](#primitive-types)
* [Optional and nullable types](#optional-and-nullable-types)
* [Property accessors](#property-accessors)
* [Naming conversion](#naming-conversion)
* [JS-name collisions with `js_sys` glob imports](#js-name-collisions-with-js_sys-glob-imports)
* [Classes](#classes)
* [Interfaces (class-like vs dictionary)](#interfaces-class-like-vs-dictionary)
* [Dictionary builders](#dictionary-builders)
* [Anonymous interface synthesis](#anonymous-interface-synthesis)
* [`var X: { new(...): T }` patterns](#var-x--new-t-patterns)
* [Module-scoped constructor variables](#module-scoped-constructor-variables)
* [Signature flattening](#signature-flattening)
* [Methods + the `try_<name>` companion](#methods--the-try_name-companion)
* [`Promise<T>` returns become `async fn`](#promiset-returns-become-async-fn)
* [`@throws` JSDoc → typed error](#throws-jsdoc--typed-error)
* [Subtyping LUB across unions](#subtyping-lub-across-unions)
* [Module declarations and namespace nesting](#module-declarations-and-namespace-nesting)
* [Type aliases and `export { X as Y }`](#type-aliases-and-export--x-as-y-)
* [String and numeric enums](#string-and-numeric-enums)
* [Multiple-context name resolution](#multiple-context-name-resolution)
* [External type mapping and web platform defaults](#external-type-mapping-and-web-platform-defaults)

---

## Primitive types

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
position vs return position. Argument-position container types are
borrowed by reference; return-position container types are owned.

## Optional and nullable types

* `T | null` → `Option<T>` in return position, `Option<T>` in argument
  position. (`null`-only is rare; treated like `undefined`.)
* `T | undefined` and `T | null | undefined` → also `Option<T>`. We coalesce
  at parse time; the rendered union has no separate `null`/`undefined`
  arm.
* `T?` on a property → `Option<T>`. The setter takes `Option<T>` too, so
  callers can clear the property by passing `None`.
* `f(x?: T)` (optional parameter) → produces an overload pair, *not* an
  `Option<T>` parameter. See [Signature flattening](#signature-flattening).

## Property accessors

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

## Naming conversion

* JS `camelCase` / `PascalCase` identifiers → Rust `snake_case` for fns,
  `PascalCase` for types.
* `js_name = "..."` is emitted whenever the Rust ident differs from the JS
  ident, so `wasm-bindgen` binds to the correct runtime name.
* Reserved Rust keywords (e.g. `type`, `match`, `move`) are emitted as raw
  identifiers (`r#type`).

## JS-name collisions with `js_sys` glob imports

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

## Classes

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

## Interfaces (class-like vs dictionary)

Interfaces are classified by shape (see `parse/classify.rs`):

* **Class-like** — has methods, used as a type: emit `pub type Foo;` plus
  member bindings, just like a class. No constructor.
* **Dictionary** — properties only, no methods, used as an options bag:
  emit `pub type Foo;` plus a Rust-side `new()` factory and (usually) a
  fluent builder. Setters/getters are still emitted as wasm-bindgen
  bindings, the builder just calls them. See [Dictionary builders](#dictionary-builders).

Multiple interface declarations with the same name + module context merge:
their members union, their `extends` lists merge.

## Dictionary builders

Required properties go in via the constructor; optional properties chain
fluently through a wrapper that ends in `build()`. Required-ness is
enforced by the type system, so neither `new` nor `build` needs to
return a `Result`.

### Why `builder(reqs)` instead of arg-free `builder()`

The common Rust idiom (`derive_builder`, `bon`, `typed-builder`) is an
arg-free `Foo::builder()` followed by fluent setters and a fallible
`build() -> Result<Foo, Error>`. Those crates take that shape because
*derive macros* can't reliably infer which fields are required without
extra annotations, so they degrade to runtime checks.

ts-gen has the required/optional split directly from the TS source
(`?` markers on each property), so we use it: required fields go in
the constructor signature, optionals stay fluent. The trade-off is one
syntactic step away from `Foo::builder().req_a(x).req_b(y).build()?`
toward `Foo::builder(x, y).build()` — but in exchange every required
field is checked at compile time and `build()` is infallible.

Precedent for the constructor-takes-required-args shape exists in
e.g. `tokio::process::Command::new(program)` and
`http::Request::Builder::method(_)`-style chains. It's not the most
common Rust idiom but it's not unprecedented — and it's the only
shape that captures the "required" half of TypeScript's optional-marker
information.



### Required + optional properties → `new(reqs)` and `builder(reqs)`

```ts
interface SendEmailMessage {
  from: string;
  to: string;
  subject: string;
  text?: string;
  html?: string;
}
```

emits:

```rust
impl SendEmailMessage {
    pub fn new(from: &str, to: &str, subject: &str) -> Self {
        Self::builder(from, to, subject).build()
    }

    pub fn builder(from: &str, to: &str, subject: &str) -> SendEmailMessageBuilder {
        let inner = <js_sys::Object as JsCast>::unchecked_into::<Self>(js_sys::Object::new());
        inner.set_from(from);
        inner.set_to(to);
        inner.set_subject(subject);
        SendEmailMessageBuilder { inner }
    }
}

pub struct SendEmailMessageBuilder { inner: SendEmailMessage }
impl SendEmailMessageBuilder {
    pub fn text(self, val: &str) -> Self { self.inner.set_text(val); self }
    pub fn html(self, val: &str) -> Self { self.inner.set_html(val); self }
    pub fn build(self) -> SendEmailMessage { self.inner }
}
```

Two call patterns:

```rust
// All required, no optionals
let msg = SendEmailMessage::new(from, to, subject);

// Required + some optionals
let msg = SendEmailMessage::builder(from, to, subject)
    .text("hi")
    .build();
```

`new(reqs)` and `builder(reqs)` always take the same arguments — `new`
is just `Self::builder(reqs).build()` for the no-optionals case.

### All-optional properties → `new()` and `builder()`

```ts
interface ResponseInit { status?: number; headers?: Headers; }
```

emits the same shape as above, but with zero-arg `new()` and `builder()`:

```rust
let init = ResponseInit::builder().status(200.0).build();
let init = ResponseInit::new();  // empty object
```

### Required-property cartesian product across union types

When a required property has union-typed setter overloads (e.g.
`from: string | EmailAddress`), each combination of overloads across
required fields produces a distinct `new*` / `builder*` pair. The
naming follows the standard
[`_with_X` / `_with_X_and_Y` rule](#signature-flattening). For
`SendEmailBuilder` with `from: string | EmailAddress` and
`to: string | string[]`:

```rust
SendEmailBuilder::new(from: &str, to: &str, subject: &str)
SendEmailBuilder::new_with_str_and_array(from: &str, to: &Array<JsString>, subject: &str)
SendEmailBuilder::new_with_email_address_and_str(from: &EmailAddress, to: &str, subject: &str)
SendEmailBuilder::new_with_email_address_and_array(from: &EmailAddress, to: &Array<JsString>, subject: &str)
// matching builder*, builder_with_*, etc.
```

### Generated doc comments

Every `new*` and `builder*` variant ships with a doc block listing
exactly what it does:

* Each baked-in literal renders as a bullet
  `` `field_name: literal_value` `` followed by the field's JSDoc (if
  any).
* Caller-provided fields land under a `# Provided fields` heading,
  one bullet per parameter, again pulling from the field's JSDoc.

For example:

```rust
/// * `disposition: "inline"`: One of "inline" (default) or "attachment"
///
/// # Provided fields
///
/// * `content`: A file attachment for an email message
/// * `filename`: ...
/// * `type`: ...
pub fn new_inline(content: &str, filename: &str, type_: &str) -> EmailAttachment
```

### Literal-type discriminator collapse

When a required property's union has string/number/boolean *literal*
members (e.g. `disposition: "inline" | "attachment"`), the literal
becomes part of the function name and the parameter is dropped. The
user picks the variant by calling the right constructor, no string
typo'ing required:

```ts
type EmailAttachment =
  | { disposition: "inline"; content: string | ArrayBuffer; filename: string; type: string }
  | { disposition: "attachment"; content: string | ArrayBuffer; filename: string; type: string };
```

emits:

```rust
EmailAttachment::new_inline(content: &str, filename: &str, type_: &str)
EmailAttachment::new_inline_with_array_buffer(content: &ArrayBuffer, filename: &str, type_: &str)
EmailAttachment::new_attachment(content: &str, filename: &str, type_: &str)
EmailAttachment::new_attachment_with_array_buffer(content: &ArrayBuffer, filename: &str, type_: &str)
```

Mixed unions like `disposition: "inline" | string` produce one variant
per literal *plus* a generic catch-all that takes the field as a
parameter:

```rust
EmailAttachment::new_inline(content, filename, type_)        // disposition baked in
EmailAttachment::new(disposition: &str, content, filename, type_)  // catch-all
```

### Has any `readonly` property → `new()` only, no builder

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

### Optional-property union setters

When an *optional* property's setter has union types, each variant
becomes a distinct builder method with the standard `_with_<type>`
suffix. Calling more than one of them on the same builder overwrites
earlier values.

```ts
interface ResponseInit {
  headers?: Headers | string[][] | Record<string, string>;
}
```

emits builder methods `headers`, `headers_with_array`,
`headers_with_record`.

## Anonymous interface synthesis

An inline `{ … }` type — or a union of `{ … }` types — that appears
in a position where a named interface would do is promoted to a real
`InterfaceDecl` so consumers get a typed builder rather than an opaque
`Object`. Two positions trigger synthesis:

### Parameter position

```ts
interface SendEmail {
  send(builder: {
    from: string | EmailAddress;
    to: string | string[];
    subject: string;
    headers?: Record<string, string>;
    // …
  }): Promise<EmailSendResult>;
}
```

is treated as if the user had written

```ts
interface SendEmailBuilder {
  from: string | EmailAddress;
  to: string | string[];
  subject: string;
  headers?: Record<string, string>;
  // …
}
interface SendEmail {
  send(builder: SendEmailBuilder): Promise<EmailSendResult>;
}
```

### Type-alias position

```ts
type R2Range = {
  offset?: number;
  length?: number;
  suffix?: number;
};
```

is treated as if the user had written `interface R2Range { … }`.
Type aliases whose target is a single inline literal — or a union of
inline literals (see below) — promote directly to interfaces; aliases
to anything else (named types, primitives, function types, generics,
`Record<…>`, etc.) keep their existing alias semantics.

### Union of inline literals

When every branch of a union is itself an inline literal, the branches
are **structurally merged** into a single interface body. The merge
covers both positions above:

```ts
type EmailAttachment =
  | { disposition: "inline";     contentId: string;    filename: string; … }
  | { disposition: "attachment"; contentId?: undefined; filename: string; … };
```

becomes a single `interface EmailAttachment { … }` whose members are
the union of every branch's properties, with optionality and types
adjusted so the merged shape is valid against every branch.

The merge rules:

* **Property optionality**: a property is required iff it is present
  and non-optional in **every** branch. If any branch declares it
  optional, or omits it entirely, the merged property is optional.
* **Property type**: the union of the branch types where the property
  appears. The resulting union goes through the regular union
  resolution — [subtyping LUB](#subtyping-lub-across-unions) when the
  members share a common ancestor, `JsValue` otherwise.
* **Read-only**: writable iff writable in every branch where it
  appears. A `readonly` declaration in any branch downgrades the
  merged property to read-only.
* **Methods of the same name**: every branch's signature survives as
  an overload, then flows through [signature flattening](#signature-flattening)
  to produce the disambiguated bindings.
* **Index signatures**: dedup by structural equality; the first one
  wins on conflict.

The synthesized type then inherits every other rule that applies to
interfaces: dictionary-vs-class-like classification (see
[Interfaces](#interfaces-class-like-vs-dictionary)), the dictionary-
builder treatment for property-only shapes (see
[Dictionary builders](#dictionary-builders)), and the union-typed
setter expansion (see [Signature flattening](#signature-flattening))
that turns `from: string | EmailAddress` into separate setter and
builder methods.

### Naming

For **parameter** position the synthesized name is
`<Parent><ParamSegment>` PascalCased:

* `<Parent>` is the surrounding interface or class name.
* `<ParamSegment>` is the parameter's own identifier (`builder` →
  `Builder`).
* Falls back to the member's JS name when the parameter is destructured
  or otherwise unnamed (e.g. `WorkflowInstance.sendEvent({ event })`
  synthesizes `WorkflowInstanceSendEvent`).

For **type-alias** position the synthesized name is the alias's own
name — `type R2Range = { … }` synthesizes `interface R2Range { … }`.

Collisions with names already in scope (user-declared types or other
synthesized types) get a numeric suffix: two methods on the same
parent both taking `(options: { … })` produce `FooOptions` and
`FooOptions2`.

### Hoisting scope

Only **directly-inline** type literals (or unions of such) are
synthesized. Anonymous types nested inside a generic, an array,
`Record<…>`, or a property of another object literal are not hoisted
— they follow the regular type-mapping rules and erase to `Object`.
Inline literals inside the *body* of a hoisted interface are themselves
hoisted recursively, using the synthesized parent's name.

## `var X: { new(...): T }` patterns

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

## Module-scoped constructor variables

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

## Signature flattening

TypeScript can describe a single callable in several ways that all mean
"there are multiple shapes of arguments this accepts": explicit
overloads, optional parameters, union-typed parameters, variadics. They
go through one shared pipeline in
`codegen::signatures::expand_signatures` so the binding names and
dedup behaviour stay consistent across the four cases.

### The four input forms

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

### The pipeline

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

### Examples

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

### Why a single pipeline

Treating optional, union, overload, and variadic as one parameter-axis
problem keeps suffix naming consistent (the `_with_X` rules apply to
every binding regardless of which input form produced it),
keeps cross-overload dedup honest (truncation collisions get dropped
once across all input forms), and keeps the per-callable layer
oblivious to the combinatorics.

An earlier design interleaved the four expansions across the codebase
and produced near-duplicate bindings whenever two of them combined.

## Methods + the `try_<name>` companion

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

## `Promise<T>` returns become `async fn`

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

## `@throws` JSDoc → typed error

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

## Subtyping LUB across unions

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

## Module declarations and namespace nesting

```ts
declare module "cloudflare:email" {
  class EmailMessage { ... }
}
interface SendEmail {
  send(message: EmailMessage): Promise<EmailSendResult>;
}
```

emits a `pub mod email { ... }` (the prefix `cloudflare:` is stripped
to the part after the last `:`; protocol prefixes like `node:` and
`cloudflare:` are dropped via
`util::naming::module_specifier_to_ident`). All bindings inside use
`#[wasm_bindgen(module = "cloudflare:email")]`.

References that cross a module boundary are emitted as **qualified
paths**, not bare idents:

* From `Global` → `Module(m)`: prefix `m::` (e.g.
  `&email::EmailMessage`).
* From `Module(m)` → `Module(n)`: prefix `super::n::` (hop up to the
  parent file scope, then down into the sibling).
* From `Module(m)` → `Global`: bare ident — the inner module's
  `use super::*;` makes parent items visible already.

Qualification keys off the *resolved* declaration's `module_context`,
not the textual name, so a global `interface Foo` and a module-scoped
`class Foo` qualify independently. The use-site scope chain picks the
visible one.

```ts
namespace WebAssembly {
  class Module { ... }
}
```

emits a `pub mod web_assembly { ... }` with `#[wasm_bindgen(js_namespace
= "WebAssembly")]` on each member. The namespace lookup is one-deep —
nested namespaces are not yet supported.

## Type aliases and `export { X as Y }`

* `type Foo = Bar;` → `pub type Foo = Bar;` if `Bar` is a recognised
  type, or chases the alias chain to its terminal during codegen.
* `export { Local as Public };` (sourceless) → recorded in
  `TypeRegistry::export_renames`. The local declaration is published
  under the public name, and any redundant alias stub is suppressed.
* `export { X as Y } from "...";` (with source) → registered as an import
  from the named module.

## String and numeric enums

```ts
enum Color { Red = "red", Green = "green" }
```

emits a `pub enum Color { Red, Green }` plus serde-aware `to_string` /
`try_from_str` impls. `wasm-bindgen` doesn't handle string enums
natively, so we lower these to Rust-side enums + a `JsValue` round-trip.

Numeric enums lower similarly with explicit discriminant values.

## Multiple-context name resolution

When the same name appears in different `ModuleContext`s (e.g. a global
`interface EmailMessage` and a `cloudflare:email`-scoped class
`EmailMessage`), they remain distinct types. `merge_class_pairs` keys on
`(name, ModuleContext)` to keep them separate. Same-context same-name
still merges as expected.

## External type mapping and web platform defaults

Names that resolve through scope but aren't declared in the input
source — `Blob`, `Headers`, `Event`, `ReadableStream`, `Response`, … —
fall through to the **external map**. The resolution order at each use
site is:

1. **`js_sys::*` glob** for the names listed in `JS_SYS_RESERVED`
   (`Error`, `Promise`, `Map`, `Array`, `Object`, …). Emitted as bare
   idents, no `use` alias needed.
2. **User-supplied `--external` mappings**, in priority order:
   explicit type maps (`Blob=::web_sys::Blob`) > module maps
   (`node:buffer=node_buffer_sys`) > wildcard module maps
   (`node:*=node_sys::*`).
3. **Built-in web platform defaults**: `Blob`, `Event`, `Headers`,
   `ReadableStream`, `Response`, `URL`, `URLSearchParams`,
   `WebSocket`, … all map to their `::web_sys::*` equivalents
   automatically. The full list lives in
   `external_map::WEB_SYS_DEFAULTS`. Run with `--no-web-sys` to
   disable these defaults (e.g. for environments that don't link
   `web_sys`).
4. **Fallback**: emit a `#[allow(dead_code)] use JsValue as Foo;`
   alias plus an error diagnostic so the output still compiles while
   surfacing the missing mapping.

User mappings always override the defaults. The `js_sys` short-circuit
is unaffected by `--no-web-sys` — those names are part of the
generated file's `use js_sys::*;` prelude.
