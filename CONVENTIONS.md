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
* [Array and slice types](#array-and-slice-types)
* [Property accessors](#property-accessors)
* [Naming conversion](#naming-conversion)
* [JS-name collisions with `js_sys` glob imports](#js-name-collisions-with-js_sys-glob-imports)
* [Classes](#classes)
* [Interfaces (class-like vs dictionary)](#interfaces-class-like-vs-dictionary)
* [Dictionary builders](#dictionary-builders)
* [Anonymous interface synthesis](#anonymous-interface-synthesis)
* [Discriminated unions](#discriminated-unions)
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

* `T | null` → `Option<T>` in return position. In argument position the
  `null` arm is dropped — the parameter takes `T` directly. A
  `_with_null(val: &Null)` overload would force callers to construct a
  `Null` value with no real upside, and the omission case for
  truly-optional params is already covered by the optional-truncation
  rule below.
* `T | undefined` and `T | null | undefined` follow the same rules as
  `T | null` — coalesced at parse time; the rendered union has no
  separate `null`/`undefined` arm.
* In inner type positions, `T | null` → `JsOption<T>` unless `T` already
  erases to `JsValue`; `JsOption<JsValue>` simplifies to `JsValue`.
* `T?` on a property → getter returns `Option<T>`; setter takes `T`.
* `f(x?: T)` (optional parameter) → produces an overload pair, *not* an
  `Option<T>` parameter. See [Signature flattening](#signature-flattening).

## Array and slice types

Top-level `Array<T>` and the syntactic `T[]` lower to Rust-idiomatic
sequences:

* **Argument position** → `&[T]`. wasm-bindgen handles the JS-side
  conversion; for primitive numeric `T` the slice arrives as a
  zero-copy typed-array view, otherwise it's materialised as a plain
  JS `Array`.
* **Return position** → `Vec<T>`.

Element type lowering inside the slice / `Vec<T>` follows
**return-position** rules regardless of the outer direction:

| TypeScript element        | Slice / Vec element    |
| ------------------------- | ---------------------- |
| `number`                  | `f64`                  |
| `bigint`                  | `i64`                  |
| `boolean`                 | `bool`                 |
| `string`                  | `String`               |
| `Foo` (named JS / Rust type) | `Foo`               |
| `any` / `unknown`         | `JsValue`              |

Strings stay owned (`Vec<String>`, not `Vec<&str>`); JS-imported
classes stay unborrowed (`&[EmailAttachment]`, not
`&[&EmailAttachment]`); primitives stay bare (`&[f64]`, not
`&[Number]`).

Inside an actual generic (`Promise<Array<T>>`, `Map<K, Array<V>>`,
…) the legacy `Array<T'>` form is preserved — `Promise<T>` /
`Map<K,V>` etc. require their generic argument to satisfy
`T: JsGeneric`, which Rust's `Vec<T>` doesn't.

### `slice_to_array` attribute

By default, `&[T]` arguments to imported JS functions arrive as a
zero-copy typed-array view when `T` is a primitive numeric (`u8`,
`i32`, `f64`, …) and as a plain JS `Array` otherwise. ts-gen wants
the `Array` representation (a TS `Array<string>` or `Array<Foo>` is
a plain JS Array, not a typed view), so it tags every binding whose
parameters include a non-numeric `&[T]` (or `Option<&[T]>`) with
`#[wasm_bindgen(slice_to_array)]`:

```rs
#[wasm_bindgen(method, slice_to_array, js_name = "acceptWebSocket")]
pub fn accept_web_socket_with_tags(this: &DurableObjectState, ws: &WebSocket, tags: &[String]);
```

The user-facing Rust signature is unchanged — `&[T]` stays `&[T]`.
Only the JS-side wire format (and the JS-visible type) changes.

ts-gen emits the attribute per-function (not per-block) — every
imported callable that has at least one qualifying parameter gets
its own `slice_to_array`. Functions whose only slice params are
numeric (`&[f64]`, `&[i64]`) keep the default typed-array
representation and do not get the attribute.

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

If every property is required (no optionals), the builder would carry
only `build()` and is suppressed — only `new(reqs)` is emitted, with
construction inlined.

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
exactly what it does. Headings use `##` (h2) and bullets use ` - ` to
separate the field name from its description, matching the format
used elsewhere when JSDoc is rendered to Rust:

* `## Inlined fields` — bullets `` `field_name: literal_value` `` for
  each literal discriminant baked into the function name (these don't
  appear as parameters).
* `## Arguments` — bullets `` `field_name` `` for each caller-supplied
  field, in signature order.

Both sections pull the field's JSDoc into the bullet when present.

For example:

```rust
/// ## Inlined fields
///
/// * `disposition: "inline"` - One of "inline" (default) or "attachment"
///
/// ## Arguments
///
/// * `content` - A file attachment for an email message
/// * `filename` - ...
/// * `type` - ...
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

### Per-branch required fields in discriminated unions

When a union qualifies as a [discriminated union](#discriminated-unions),
each `new_<discriminator>` / `builder_<discriminator>` factory derives
its required-field set from **its own branch**, not from the merged
shape. For

```ts
type EmailAttachment =
  | { disposition: "inline";     contentId: string;     filename: string; type: string; content: string | ArrayBuffer; }
  | { disposition: "attachment"; contentId?: undefined; filename: string; type: string; content: string | ArrayBuffer; };
```

`contentId` is required in the `inline` branch and absent (`?:
undefined`) in the `attachment` branch. The factory pair reflects
that:

```rust
EmailAttachment::new_inline(content_id: &str, filename: &str, type_: &str, content: &str)
EmailAttachment::new_attachment(filename: &str, type_: &str, content: &str)
```

The builder wrapper (`EmailAttachmentBuilder`) still exposes a fluent
`content_id(self, val)` setter — it reflects the merged-shape view
that the field is optional on the type as a whole, so callers going
through `builder_attachment(...).content_id(x).build()` aren't
prevented from setting it. The branch invariant is enforced at the
`new_<discriminator>` boundary, not inside the builder.

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

emits builder methods `headers`, `headers_with_slice`,
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
are **structurally merged** into a single member set. The merge
covers both positions above:

```ts
type EmailAttachment =
  | { disposition: "inline";     contentId: string;    filename: string; … }
  | { disposition: "attachment"; contentId?: undefined; filename: string; … };
```

The merge rules apply to the unified shape:

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

The synthesized type lands in one of two kinds depending on whether a
discriminator is detected (see [Discriminated unions](#discriminated-unions)):

* **`InterfaceDecl`** when no shared literal-typed required field
  exists — falls through the regular dictionary-vs-class-like
  pipeline, the dictionary-builder treatment for property-only
  shapes, and the union-typed setter expansion that turns
  `from: string | EmailAddress` into separate setter and builder
  methods.
* **`DiscriminatedUnionDecl`** when at least one shared literal-typed
  required field exists — gets the per-branch factory rules described
  in the next section.

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

## Discriminated unions

A type alias `type Foo = A | B | …` whose branches share at least one
**required, literal-typed** property is promoted to a
`DiscriminatedUnionDecl` rather than a plain `InterfaceDecl`.

A property qualifies as a discriminator when, in **every** branch, it
is present, required (no `?:` marker), and typed as a string, number,
or boolean literal. TypeScript's narrowing accepts all three; the
codegen rule matches.

```ts
// String discriminator
type EmailAttachment =
  | { disposition: "inline";     contentId: string;     … }
  | { disposition: "attachment"; contentId?: undefined; … };

// Boolean discriminator
type ReadableStreamReadResult<R = any> =
  | { done: false; value: R }
  | { done: true;  value?: undefined };

// Number discriminator
type Status =
  | { code: 200; body: string }
  | { code: 404; reason: string };
```

### Codegen shape

The extern block is the same as for any other dictionary — a single
`pub type Foo;` plus getter/setter bindings for the merged shape.

The `impl Foo` block emits **per-branch** `new_*` and (when useful)
`builder_*` factories. Each variant's required positional parameters
are derived from **its own branch**, not from the merged shape — so
the `EmailAttachment` `inline` branch's `contentId: string` shows up
as a required argument in `new_inline(...)` even though the merged
shape marks `contentId` optional.

```rust
EmailAttachment::new_inline(content_id: &str, filename: &str, type_: &str, content: &str)
EmailAttachment::new_inline_with_array_buffer(content_id: &str, filename: &str, type_: &str, content: &ArrayBuffer)

EmailAttachment::new_attachment(filename: &str, type_: &str, content: &str)
EmailAttachment::new_attachment_with_array_buffer(filename: &str, type_: &str, content: &ArrayBuffer)
```

The literal-collapse, value-union expansion, and `_with_<type>`
suffixing rules described in [Dictionary builders](#dictionary-builders)
all apply *within each branch* — branches don't influence each
other's suffix decisions.

### Wrapper builder remains merged

The wrapper struct (`EmailAttachmentBuilder`) and its fluent setters
are computed from the **merged-shape optional set**, not per branch.
For `EmailAttachment` this means
`EmailAttachmentBuilder::content_id(self, val)` is always available,
even after `builder_attachment(...)`. The branch invariant is enforced
at the `new_<discriminator>` boundary; once you're past that, the
wrapper exposes the runtime-permissive view of the type. Calling
`.content_id(x)` after `builder_attachment(...)` writes through to
the JS object exactly as `set_content_id` would on the bare type —
no special branch enforcement.

### `builder_<branch>` suppression

A `builder_<branch>` is only emitted when **at least one merged-optional
field isn't already required by that branch**. If a branch's required
set covers every merged-optional field, going through the wrapper
would have nothing to chain — so the `new_<branch>` body is inlined
directly:

```rust
// inline branch covers `contentId` (the only merged-optional), so no builder_inline:
EmailAttachment::new_inline(content_id, filename, type_, content) {
    let inner: Self = JsCast::unchecked_into(js_sys::Object::new());
    inner.set_disposition("inline");
    inner.set_content_id(content_id);
    // …
    inner
}

// attachment branch leaves `contentId` to the wrapper, so builder_attachment exists:
EmailAttachment::new_attachment(filename, type_, content) {
    Self::builder_attachment(filename, type_, content).build()
}
```

This collapses to the existing all-required dictionary rule (see
[Dictionary builders](#dictionary-builders)) when the type has no
optional fields at all — there's nothing to chain, so `new(reqs)` is
emitted with an inlined body and no builder type is generated.

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
   Unions inside generic type arguments do not distribute:
   `Array<A | B>` and `Record<K, A | B>` are each one parameter shape,
   not `Array<A> | Array<B>` or `Record<K, A> | Record<K, B>`.
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

### `_with_<type>` suffix vocabulary

When a union expansion drives the suffix (same parameter name,
different types), the suffix mirrors the **Rust** lowering rather
than the TypeScript spelling, so the resulting binding name lines
up with what callers see in the function signature:

| TypeScript      | Rust (arg)       | Suffix          |
| --------------- | ---------------- | --------------- |
| `string`        | `&str`           | `_with_str`     |
| `number`        | `f64`            | `_with_f64`     |
| `boolean`       | `bool`           | `_with_bool`    |
| `bigint`        | `BigInt`         | `_with_big_int` |
| `Array<T>` / `T[]` | `&[T]`        | `_with_slice`   |
| `Uint8Array`    | `&Uint8Array`    | `_with_uint8_array` |
| `Foo` (named)   | `&Foo`           | `_with_foo`     |
| `null`          | (dropped — see [Optional and nullable types](#optional-and-nullable-types)) | – |
| `undefined`     | (dropped, same)  | – |
| `any` / `unknown` | `&JsValue`     | `_with_js_value`|

For named JS types the snake-cased head doubles as the Rust
identifier (`Uint8Array` → `uint8_array`, also `&Uint8Array` in
the rendered type), so there's no separate translation table —
the same convention works in both directions.

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

### Async return primitives lower to `js_sys` wrappers

Primitive types behave differently in `Promise<T>` than they do in
sync returns or arguments:

| TypeScript                     | Async return                  |
| ------------------------------ | ----------------------------- |
| `Promise<boolean>`             | `Result<Boolean, JsValue>`    |
| `Promise<number>`              | `Result<Number, JsValue>`     |
| `Promise<string>`              | `Result<JsString, JsValue>`   |
| `Promise<void>`                | `Result<Undefined, JsValue>`  |
| `Promise<T \| null>`           | `Result<JsOption<T>, JsValue>`|
| `Promise<Foo>` (named JS type) | `Result<Foo, JsValue>`        |

`wasm-bindgen`'s typed `Promise<T>` / `JsFuture<T>` require
`T: JsGeneric` — an externref-backed type — which bare Rust
primitives aren't. The `js_sys` wrappers are. Callers recover Rust
primitives via `value_of()` (for `Boolean` / `Number`) or
`String::from(_)` / `.into()` (for `JsString`).

Sync returns, arguments, and properties keep the bare-primitive
lowering.

## `Iterator<T>` / `IterableIterator<T>` map to `js_sys::Iterator<T>`

Already-iterator types — anything that exposes the `next()` /
`{value, done}` protocol directly — map straight to the
`wasm-bindgen` typed iterator wrappers. Both the sync and async
families share a single dispatch:

| TypeScript               | Rust                            |
| ------------------------ | ------------------------------- |
| `Iterator<T>`            | `js_sys::Iterator<T>`           |
| `IterableIterator<T>`    | `js_sys::Iterator<T>`           |
| `AsyncIterator<T>`       | `js_sys::AsyncIterator<T>`      |
| `AsyncIterableIterator<T>` | `js_sys::AsyncIterator<T>`    |

The inner `T` lowers like any other generic argument: type
parameters lower to a bare ident (with the surrounding method or
type carrying a `<T: ::wasm_bindgen::JsGeneric>` bound), tuples
become `ArrayTuple<…>`, primitives become their JS wrapper forms.

## `Iterable<T>` returns synthesize a wrapper with `[Symbol.iterator]()`

`Iterable<T>` is the **protocol** — an object exposing
`[Symbol.iterator](): Iterator<T>`, distinct from the iterator
itself. wasm-bindgen has no inline way to express this, so top-level
occurrences in return position are hoisted into a synthesized extern
type:

```ts
interface SyncKvStorage {
  list<T>(): Iterable<[string, T]>;
}
```

becomes:

```rust
pub type SyncKvStorageList<T: ::wasm_bindgen::JsGeneric>;

#[wasm_bindgen(method, js_name = "[Symbol.iterator]")]
pub fn iterator<T: ::wasm_bindgen::JsGeneric>(
    this: &SyncKvStorageList<T>,
) -> Iterator<ArrayTuple<(JsString, T)>>;

#[wasm_bindgen(method)]
pub fn list<T: ::wasm_bindgen::JsGeneric>(this: &SyncKvStorage) -> SyncKvStorageList<T>;
```

The wrapper's name follows the existing `<Parent><Member>`
convention (with dedup on collision), mirroring anonymous-interface
parameter synthesis. `AsyncIterable<T>` synthesizes the analogous
wrapper keyed on `[Symbol.asyncIterator]`. The bracketed `js_name`
form matches wasm-bindgen's computed-property syntax for symbol-keyed
methods. Nested occurrences (inside unions, arrays, etc.) are not
synthesized — they erase to `JsValue`, matching the existing
parameter-synthesis limitation.

Type parameters mentioned by the iteration item bubble up onto the
synthesized wrapper, so `Iterable<[string, T]>` produces
`<Parent>List<T>` rather than erasing `T`.

## In-scope generic type parameters

Bare type-parameter references (`T` in `put<T>(value: T)`) survive
codegen as `<T: ::wasm_bindgen::JsGeneric>` declarations rather than
being erased:

```ts
interface KeyValueStore {
  put<T>(key: string, value: T): void;
  get<T = unknown>(key: string): T | undefined;
}
```

becomes:

```rust
pub fn put<T: ::wasm_bindgen::JsGeneric>(this: &KeyValueStore, key: &str, value: &T);
pub fn get<T: ::wasm_bindgen::JsGeneric>(this: &KeyValueStore, key: &str) -> Option<T>;
```

Parse-time, every type-parameter-bearing declaration (class,
interface, type alias, method, function, namespace) creates a child
**body scope** with its `<T, ...>` bound as
`Binding::TypeParam`. Codegen consults `scopes.resolve_binding` at
each name-emission site — when a name resolves to a type parameter,
it lowers to a bare Rust ident; otherwise it goes through the
regular declared-type / external-map / `JsValue` fallback path.

Methods redeclare every type parameter mentioned in their signature,
even those inherited from the surrounding type. `js_sys` follows the
same convention (see `js_sys::Array::for_each<T: JsGeneric>`).

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
* `@throws {never}` — declares the callable never throws (see below)
* Multiple `@throws` lines aggregate into one effective union
* `@throws Sentence describing condition.` — pure prose, no structured
  type extracted

The original prose surfaces in the rendered doc as an `## Errors` section.

### `--errors-as-error` — typed default error

Without `@throws`, fallible bindings default to `Result<T, JsValue>`.
The `--errors-as-error` flag (or `GenerateOptions::errors_as_error`
for library callers) flips that default to `Result<T, Error>`
(`js_sys::Error`). Bindings whose `@throws` JSDoc names a specific
type still use that type — the flag is purely about the *default*.

```ts
upload(file: File): Promise<void>;
```

emits

```rust
// default
pub async fn upload(this: &Foo, file: &File) -> Result<Undefined, JsValue>;
// with --errors-as-error
pub async fn upload(this: &Foo, file: &File) -> Result<Undefined, Error>;
```

Useful in environments that prefer the typed `Error` API
(`message`, `name`, `cause`, …) over raw `JsValue`. The trade-off
is that JS code throwing a non-Error value (`throw "oops"`) now
fails the conversion at the FFI boundary; if you can't audit your
sources for that, stay with `JsValue`.

### `@throws {never}` — opting out of fallible variants

`@throws {never}` is the explicit "this never throws" annotation. It
combines TypeScript's bottom type with JSDoc to tell ts-gen to skip
the fallible variants for the callable:

```ts
class NeverThrows {
  /**
   * @throws {never}
   */
  safeOp(x: number): string;

  /**
   * @throws {never}
   */
  loaded(): Promise<Loaded>;
}

/**
 * @throws {never}
 */
declare function pureCompute(x: number): number;
```

emits:

```rust
// Sync methods/functions: no `try_<name>` companion.
#[wasm_bindgen(method, js_name = "safeOp")]
pub fn safe_op(this: &NeverThrows, x: f64) -> String;

// Async methods/functions: no `Result` wrapper, no `catch`.
#[wasm_bindgen(method)]
pub async fn loaded(this: &NeverThrows) -> Loaded;

#[wasm_bindgen(js_name = "pureCompute")]
pub fn pure_compute(x: f64) -> f64;
```

Compare to a plain sync method, which emits both forms:

```rust
pub fn frobnicate(this: &Foo) -> String;             // panic on throw
pub fn try_frobnicate(this: &Foo) -> Result<String, JsValue>;
```

#### Rules

* **Sync callables** with `@throws {never}` get *only* the primary
  binding — no `try_<name>` companion is emitted.
* **Async callables** (returning `Promise<T>`) drop the `Result<T, _>`
  wrapper and the `catch` attribute. The binding becomes
  `pub async fn foo() -> T`.
* **Constructors** always catch per JS `new` semantics —
  `@throws {never}` on a constructor is silently ignored.
* **Setters** never throw and never get `try_` variants regardless,
  so the annotation is a no-op there too.
* **Mixed annotations** like `@throws {never | TypeError}` collapse to
  `@throws {TypeError}` (since `T | never` is just `T` in TS) — the
  `never` arm is absorbed and the residual type drives codegen as
  usual.
* **Rendered doc**: `@throws {never}` lines do *not* contribute to
  the `## Errors` section. By definition there are no errors to
  document.

## Subtyping LUB across unions

`TypeRef::Union` resolution applies a Least Upper Bound across its members
based on the subtyping lattice in `codegen::subtyping`:

```text
TypeError                            -> Error
TypeError | RangeError               -> Error      (both subclass Error)
TypeError | string                   -> JsValue    (no shared ancestor below Object)
BadRequestError | NotFoundError      -> StreamError (when both extend StreamError)
"inline" | "attachment"              -> string     (literal-type widening)
1 | 2 | 3                            -> number
true | false                         -> boolean
"foo" | string                       -> string     (literal joined with primitive)
```

The lattice is built from:

* A static `BUILTIN_PARENTS` table for JS Error / DOM / collection /
  typed-array hierarchies.
* User-declared `class extends X` / `interface extends X` chains, walked
  through the codegen scope.

When the deepest common ancestor is `Object` (no useful narrowing), the
union erases to `JsValue` — the existing default. This rule is universal:
it applies to `@throws` unions and to any TS union return type.

This LUB rule applies when lowering an actual union type. It does not
make generic containers distributive: `Array<TypeError | RangeError>`
is still a single `Array<Error>` type, and `Record<string, string |
number | boolean>` lowers as one `Object` binding rather than separate
record overloads for each value type.

### Erased return-position unions become dynamic-union enums

When a top-level return-position type is a union that would
otherwise erase to `JsValue` (no useful LUB), ts-gen synthesises a
`#[wasm_bindgen]` enum and uses it as the return type. The enum's
variants are tuple variants — one per TS union member — whose
payload type is the regular return-position lowering of that
member. wasm-bindgen handles dispatch at the JS↔Rust boundary by
trying the variants in source order; see [Dynamic Unions in the
wasm-bindgen guide](https://rustwasm.github.io/wasm-bindgen/reference/types/enums.html#dynamic-unions).

```ts
interface EmailAttachment {
  readonly content: string | ArrayBuffer | ArrayBufferView;
}
```

emits:

```rust
#[wasm_bindgen]
pub enum EmailAttachmentContentKind {
    String(String),
    ArrayBuffer(ArrayBuffer),
    ArrayBufferView(Uint8Array),
}

// inside the extern block:
pub fn content(this: &EmailAttachment) -> EmailAttachmentContentKind;
```

#### Naming

The enum gets a `Kind` suffix (Rust idiom for discriminated-enum
types). The base name is built from:

* **Getters** — the property's JS name. `content` → `ContentKind`.
* **Methods / functions** — `<FnName>Return`. `fetch()` →
  `FetchReturnKind` (the `Return` infix disambiguates from the
  callable's own name).

Identity is the **ordered member list**, so two erasing unions with
the same member set in the same order share a single synthesised
enum (and hence a single name). Order matters at runtime — the
wasm-bindgen dispatch tries variants in source order, so two unions
that differ in order are distinct types.

Collisions resolve in three steps, most-simple to most-qualified:

1. Bare anchor (`ContentKind`).
2. Parent-prefixed (`EmailAttachmentContentKind`) — when a different
   identity already claimed the bare anchor.
3. Numeric suffix (`ContentKind2`, `EmailAttachmentContentKind2`) —
   when both bare and parent-prefixed are taken.

First-seen wins on the bare anchor; subsequent distinct unions get
parent-prefixed (or numeric-suffixed if that also collides).

#### When synthesis fires

Three layered rules decide whether a return-position union becomes
a synthesised enum:

1. **Pure-boolean-literal unions** (`true | false`, `true`, `false`)
   keep `bool` — every member round-trips through it without loss.
2. **Any literal member** (string / number) ⇒ synthesise. Pure
   string-literal unions (`"a" | "b"`) become string-discriminant
   enums; mixed `"a" | string` becomes a dynamic union with literal
   variants + a fallback tuple. Same for `number` / `bigint`.
3. **Otherwise**, fall back to the named LUB lattice — synthesise
   only when there's no useful narrowing. Unions like
   `TypeError | RangeError` keep the `Error` ancestor; mixed
   `string | ArrayBuffer | ArrayBufferView` synthesises.

Nested unions inside generics (e.g. `Promise<32 | "foo">`,
`Array<32 | "foo">`) still go through the inner-position lowering —
the synthesised-enum path doesn't reach into generic containers
because wasm-bindgen requires `T: JsGeneric` for `Promise<T>` /
`Map<K, V>` / etc. and a synthesised enum doesn't qualify.

#### Variant kinds and naming

A synthesised enum can mix two variant forms in the same body:

* **Literal-discriminant variants** for string / number / boolean
  literal members. The variant name is PascalCased from the
  literal value; the discriminant is the literal itself, which
  wasm-bindgen routes as the literal value at the FFI boundary.
  ```rs
  pub enum RoleKind {
      User = "user",
      Assistant = "assistant",
      System = "system",
  }
  ```
* **Tuple variants** for everything else. The variant name is the
  Rust payload type; the payload is the regular return-position
  lowering of the TS type:

  | TypeScript        | Rust (return)   | Variant         |
  | ----------------- | --------------- | --------------- |
  | `string`          | `String`        | `String(String)` |
  | `number`          | `f64`           | `F64(f64)`      |
  | `bigint`          | `BigInt`        | `BigInt(BigInt)` |
  | `boolean`         | `bool`          | `Bool(bool)`    |
  | `ArrayBuffer`     | `ArrayBuffer`   | `ArrayBuffer(ArrayBuffer)` |
  | `ArrayBufferView` | `Uint8Array`    | `Uint8Array(Uint8Array)` |
  | `Array<Foo>`      | `Vec<Foo>`      | `VecOfFoo(Vec<Foo>)` |
  | `Foo` (named)     | `Foo`           | `Foo(Foo)`      |

Mixed unions like `"user" | "system" | (string & NonNullable<unknown>)`
emit literal variants for each named string + a tuple
`JsValue(JsValue)` fallback for the residual — wasm-bindgen tries
the literal arms first, and falls back through the tuple chain in
declaration order.

`ArrayBufferView` is a TS-only union alias for the typed-array
family + `DataView`, so there's no single Rust type that captures
the shape. We specialise:

* Return position (including dynamic-union variants) →
  `Uint8Array`. The most useful concrete typed-array; callers can
  re-cast to a different typed-array via `JsCast::dyn_into` if
  needed.
* Argument position → `&Object`, with the dictionary-builder path
  switching to a generic `<T: TypedArray>` when applicable.

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
