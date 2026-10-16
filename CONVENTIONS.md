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
* [Outer-type ABI fan-out](#outer-type-abi-fan-out)
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
| `bigint`                        | `BigInt`                   |
| `symbol` / `unique symbol`      | `Symbol`                   |
| `void`                          | `()` (or omitted from sig) |
| `undefined`                     | `Undefined`                |
| `null`                          | `()`                       |
| `any` / `unknown`               | `JsValue`                  |
| `never`                         | `JsValue`                  |
| `object`                        | `Object`                   |

`String` vs `&str` (or `Object` vs `&Object` etc.) is chosen by argument
position vs return position. Argument-position container types are
borrowed by reference; return-position container types are owned.

TypeScript's built-in `PropertyKey` is recognised without a declaration
and lowers exactly like `string | number | symbol`, except for
[primitive union arguments](#primitive-union-arguments) in
per-monomorphization mode.

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

A top-level `T[]` lowers to a Rust-idiomatic sequence; `Array<T>`
written as a generic stays the `Array<U>` handle:

* **Argument position** → `&[T]`, which JS receives as a plain `Array`
  (see [`slice_to_array`](#slice_to_array-attribute)).
* **Return position** → `Vec<T>`.

wasm-bindgen only has slice / `Vec` ABIs for Rust-native `String` and
`f64` elements and for JS handles. So an element that resolves, through
aliases and unions, to a string or number keeps its return-position
form; every other element takes its inner-position handle form:

| TypeScript element           | Slice / Vec element    |
| ---------------------------- | ---------------------- |
| `number`, `1 \| 2`           | `f64`                  |
| `string`, `"a" \| "b"`       | `String`               |
| `Foo` (named JS / Rust type) | `Foo`                  |
| `any` / `unknown`            | `JsValue`              |
| `boolean`                    | `Boolean`              |
| `T \| null`                  | `JsOption<U>`          |
| `T[]`                        | `Array<U>`             |

Strings stay owned (`Vec<String>`, not `Vec<&str>`); JS-imported
classes stay unborrowed (`&[EmailAttachment]`, not
`&[&EmailAttachment]`); numbers stay bare (`&[f64]`, not
`&[Number]`). With `type Row = string[]`, `Row[]` is
`&[Array<JsString>]` / `Vec<Array<JsString>>`, the same as
`string[][]`.

Inside an actual generic (`Promise<Array<T>>`, `Map<K, Array<V>>`,
…) the legacy `Array<T'>` form is preserved — `Promise<T>` /
`Map<K,V>` etc. require their generic argument to satisfy
`T: JsGeneric`, which Rust's `Vec<T>` doesn't.

### `slice_to_array` attribute

A TS `T[]` parameter is a plain JS `Array`, which the callee may keep
after the call returns. wasm-bindgen gives a slice of JS handles that
form already, but a slice of Rust values needs
`#[wasm_bindgen(slice_to_array)]`: without it `&[String]` has no ABI,
and `&[f64]` arrives as a `Float64Array` view of linear memory. So
every binding with a `&[String]` or `&[f64]` parameter (or
`Option<&[…]>`) gets the attribute, after resolving aliases, literal
unions and enums:

```rs
#[wasm_bindgen(method, slice_to_array, js_name = "acceptWebSocket")]
pub fn accept_web_socket_with_tags(this: &DurableObjectState, ws: &WebSocket, tags: &[String]);
```

A slice of handles (`&[Boolean]`, `&[WebSocket]`, `&[T]`) does not:
it would only add a per-element clone, and wasm-bindgen rejects the
attribute when the element is a type parameter. Variadic rest
parameters (`&[JsValue]`) don't either.

The user-facing Rust signature is unchanged — only the JS-side wire
format changes. The attribute is per function, so one qualifying
parameter applies it to every slice parameter of that binding.

## Outer-type ABI fan-out

Some Rust-idiomatic outer-type lowerings imply a copy across the
Wasm boundary for large data:

* `&str` arguments incur a UTF-8 conversion vs the JS-side
  `String`/`JsString` representation.
* `&[String]` / `&[f64]` arguments are copied into a JS `Array` via
  the `slice_to_array` attribute.

Rather than emit dual variants for every callable, ts-gen
**defers to the TS author** to pick the form. Each ABI choice
is exposed as a distinct TS spelling:

| TS arg form        | Rust arg-pos       | Rust return-pos | Notes                                  |
| ------------------ | ------------------ | --------------- | -------------------------------------- |
| `string`           | `&str`             | `String`        | Idiomatic Rust; UTF-8 copy on boundary |
| `String`           | `&JsString`        | `JsString`      | Zero-copy JS handle                    |
| `T[]`              | `&[T]`             | `Vec<T>`        | Idiomatic Rust slice / owned vec       |
| `Array<T>`         | `&Array<U>`        | `Array<U>`      | Zero-copy JS array handle              |
| `ReadonlyArray<T>` | `&Array<U>`        | `Array<U>`      | Same as `Array<T>` for FFI purposes    |

Where `U` is the element's inner-position lowering (`JsString`
for `string`, `Number` for `number`, `Foo` for a named ref).

Writing `Array<T>` (the explicit generic) gets the JS-array
reference form; writing `T[]` (the syntactic shortcut) gets
the Rust slice form. Writing `String` (the JS wrapper class) gets
`&JsString`; writing `string` (the primitive) gets `&str`. The
TS spelling controls the choice 1:1 — no fan-out, no duplicate
variants.

Other arg lowerings are independent of this rule:

* `&Foo` / `&Uint8Array` / `&Headers` — already JS references.
* `bool` / `f64` / `i64` — pass-by-value primitives.

### Why TS-spelling control rather than fan-out

An earlier design emitted both `&str` and `&JsString` variants
of every `string`-taking callable (`new_with_body` plus
`new_with_js_string`), letting the *caller* pick at the call
site. That doubled the API surface for every fan-out-eligible
arg and cartesian-multiplied when combined with union
expansion — a 13-required-field dict (Cloudflare worker-types'
`AIGatewayHeaders`) ballooned to 279,936 variants. Deferring to
the TS author keeps each callable's surface compact and makes
the binding's perf characteristics part of the TS contract.

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
#[wasm_bindgen(method, setter)]
pub fn set_baz(this: &Foo, val: f64);
```

`readonly` properties get a getter only. Non-readonly properties get both;
the setter is named `set_<snake_case>`.

## Naming conversion

* JS `camelCase` / `PascalCase` identifiers → Rust `snake_case` for fns,
  `PascalCase` for types.
* The statics that `declare const` / `let` / `var` lower to keep the
  same `snake_case` conversion (`caches` → `pub static caches`), with
  `#[allow(non_upper_case_globals)]` on the item; wasm-bindgen applies
  the attribute to the expanded static.
* A redeclared static (`declare var console` in several input files) is
  emitted once, from its first declaration, including that declaration's
  doc comment. Distinct JS bindings whose names collide after conversion
  are numbered in declaration order: `fooBar` and `FOO_BAR` become
  `foo_bar` and `foo_bar_2`. The same name under different
  `module = "..."` specifiers is a distinct binding.
* Reserved Rust keywords in fn, method, and accessor names get a trailing
  underscore (`type()` → `type_`). Elsewhere (parameters, variables) they
  are emitted as raw identifiers (`r#type`, `r#loop`), except `self`,
  `Self`, `super`, and `crate`, which cannot be raw and become `self_` etc.
* Names that are not valid Rust identifiers are sanitised (`0` → `_0`,
  `$public` → `public`, `@cf/openai/whisper` → `cfopenaiwhisper`).
* `js_name = "..."` is emitted whenever the final Rust ident (after all of
  the above) no longer spells the JS name, so `wasm-bindgen` binds to the
  correct runtime name. This applies to types, methods, free functions,
  variables, getters, and setters (whose JS name `wasm-bindgen` infers by
  stripping `set_`, so `set_0` needs no `js_name` but `set_public` for
  `$public` does). Raw identifiers already bind to their unraw name.
* `wasm-bindgen` resolves the JS class of constructors and static members
  from the return type / `static_method_of` ident and the item's own
  `js_namespace`, ignoring the type's attributes. So when a type's Rust
  ident differs from its JS name (`$Store` → `Store`, or a collision rename
  like `Global_`) they carry `js_class = "<js name>"`, and inside a
  namespace they repeat `js_namespace`.

## JS-name collisions with `js_sys` glob imports

The generated preamble does `use js_sys::*;`, which brings every `js_sys`
type into scope. A locally declared type with a colliding name (e.g.
`WebAssembly.Global` vs `js_sys::Global`) would be ambiguous at every
reference. We resolve this by:

1. Picking a suffixed Rust ident (`Global` → `Global_`) for the
   internal declaration.
2. Keeping `js_name = "Global"` on the type, and `js_class = "Global"` on
   its constructors and static members, so the JS-side binding is
   unaffected (see [Naming conversion](#naming-conversion)).
3. Re-exporting under the original name so the public Rust path is
   unchanged: `pub use Global_ as Global;`

```rust
pub mod web_assembly {
    use super::*;
    #[wasm_bindgen]
    extern "C" {
        #[wasm_bindgen(extends = Object, js_name = "Global", js_namespace = "WebAssembly")]
        pub type Global_;
        #[wasm_bindgen(constructor, js_class = "Global", js_namespace = "WebAssembly", catch)]
        pub fn new(...) -> Result<Global_, JsValue>;
    }
    pub use Global_ as Global;   // public face
}
```

Consumers always write `web_assembly::Global`. The `_` suffix is an
internal detail.

The bindings also spell the prelude's `Option`, `Result`, `String` and
`Vec` bare, so a declaration with one of those names is suffixed the same
way. It is not re-exported, since `pub use Result_ as Result;` would
shadow the prelude `Result` again, so consumers write `Result_`.

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

A number literal is baked in as a float literal matching the `f64`
setter, and names its factory with `.` and a leading `-` spelled out
(`scale: -1 | 0.5` → `new_neg_1()` setting `-1.0`, `new_0_5()` setting
`0.5`).

A literal that the field's [setter takes through the getter's
enum](#literal-union-setters) is passed as that enum's variant instead
(`mode: true | "x" | "y"` → `new_x()` calling
`set_mode_with_mode_kind(ModeKind::X)`).

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

### Literal union setters

A property's getter returns the
[synthesised enum](#erased-return-position-unions-become-dynamic-union-enums)
for a union of literals, so its setter takes that enum for the literal
members the enum keeps as their own variants: string literals, and every
member of a C-style integer enum. The other members keep their per-type
overloads. The enum takes the position of its first literal member for
`_with_<type>` naming, and the getter's value can be passed straight back.

```ts
interface Options {
  mode: true | "x" | "y";
  inline: "x" | "y";
  level: 0 | 1 | 2;
}
```

```rust
pub fn mode(this: &Options) -> ModeKind;
pub fn set_mode(this: &Options, val: bool);
pub fn set_mode_with_mode_kind(this: &Options, val: ModeKind);
pub fn set_inline(this: &Options, val: InlineKind);
pub fn set_level(this: &Options, val: LevelKind);
```

* A widened `&str` or `f64` setter would accept values outside the union,
  the enum does not.
* String literals covered by a `string` member stay on the `&str` setter,
  which is already exact: `mode: "x" | string` keeps `set_mode(&str)`.
* Number and boolean literals in a mixed union widen inside the enum too
  (`F64(f64)`, `Bool(bool)`), so they keep their `f64` / `bool` overloads.
* Unions without literal members keep their per-type overloads.
* Builder methods for optional fields follow their setter.

## Anonymous interface synthesis

An inline `{ … }` type — or a union or intersection built from `{ … }`
types and named references — that appears in a position where a named
interface would do is promoted to a real `InterfaceDecl` so consumers
get a typed builder rather than an opaque `Object`. Two positions
trigger synthesis:

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

is treated as if the user had written `interface R2Range { … }`, and
the alias's type parameters become the interface's. Type aliases whose
target is a single inline literal, a union of inline literals, or an
intersection (see below) promote directly to interfaces; aliases to
anything else (named types, primitives, function types, generics,
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

### Intersections

`A & B & { … }` is exactly what the user would have written as
`interface X extends A, B { … }`, so that is what it becomes: every
named reference in the intersection is an `extends` parent and every
inline literal contributes own members. The result then flows through
the regular interface pipeline, including `extends = A, extends = B`
on the wasm-bindgen side and the dictionary builder for the own
members. This is what makes the common required-field refinement
produce a usable type:

```ts
interface R2Bucket {
  get(key: string, options: R2GetOptions & { onlyIf: R2Conditional | Headers }): …;
  get(key: string, options?: R2GetOptions): …;
}
type D1Result<T = unknown> = D1Response & { results: T[] };
```

```rust
#[wasm_bindgen(extends = R2GetOptions, extends = Object)]
pub type R2BucketOptions;
pub fn new(only_if: &R2Conditional) -> R2BucketOptions;
pub fn new_with_headers(only_if: &Headers) -> R2BucketOptions;

#[wasm_bindgen(extends = D1Response, extends = Object)]
pub type D1Result<T>;
pub fn results<T>(this: &D1Result<T>) -> Vec<T>;
```

Several literals in one intersection are merged with the **meet** of
the union rules above (a value is every branch at once): a property is
required if any branch requires it, writable if any branch allows
writing, and a property typed differently across branches carries the
intersection of those types, which is bounded at codegen (see
[Intersections take their most specific member](#intersections-take-their-most-specific-member)).

Every parent must be something an interface could `extends`. A type
parameter (`T & { id: string }`) or an alias to a non-object type
(`type Id = string`) leaves the intersection unhoisted, to be bounded
at codegen. Imported and `--external` names have no local binding and
are allowed through. A parent that erases at codegen (a conditional
type, or a name that never resolves and is reported) is dropped from
`extends` rather than replaced, so the first real parent stays the
`Deref` target; the literal's own members survive either way. A generic first parent is
spelled bare in `extends`, which wasm-bindgen reads as
`Parent<JsValue>`, so such a type derives `PartialEq` without `Eq`.

In parameter position an intersection of named types alone
(`refine(value: Derived & Base)`) is not hoisted: a fresh subtype would
force callers to cast a value they already hold, and the bound names
the member the value is (`&Derived`). An alias supplies a name of its
own, so `type Person = HasName & HasAge` does hoist, with no own
members.

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

The TS lib spelling, where the static side is a named interface rather
than an inline literal, merges the same way:

```ts
interface URIError extends Error { … }
interface URIErrorConstructor {
  new (message?: string): URIError;
  readonly prototype: URIError;
}
declare var URIError: URIErrorConstructor;
```

`URIErrorConstructor` qualifies by having a construct signature and/or
a `prototype` member; it supplies the constructor and the statics, and
is not emitted as a type of its own. Declaration order does not matter.
If something else refers to `URIErrorConstructor` by name it falls back
to `JsValue` with an unresolved-type diagnostic, like any unmapped type.

A construct signature on any other object type (`interface Factory {
new (): Widget }`, or an inline `{ new (): T }` parameter type) is the
type of a constructor *value*, not a constructor of that type. It is
dropped with a warning rather than emitted as
`#[wasm_bindgen(constructor)]` on the wrong class.

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

Recognised as a module-scoped class declaration. When
`cloudflare:email` is listed in `--export`, it is emitted in the
module's `pub mod`, with its `#[wasm_bindgen(module = "cloudflare:email")]`
attribute attached to the extern block:

```rust
pub mod cloudflare {
    pub mod email {
        #[wasm_bindgen(module = "cloudflare:email")]
        extern "C" {
            pub type EmailMessage;
            #[wasm_bindgen(constructor, catch)]
            pub fn new(from: &str, to: &str, raw: &str) -> Result<EmailMessage, JsValue>;
            // ...
        }
    }
}
```

References from elsewhere are qualified, `cloudflare::email::EmailMessage`. See
[Module declarations and namespace
nesting](#module-declarations-and-namespace-nesting) for the
`--export` / `--external` knobs.

The `export { _EmailMessage as EmailMessage }` rename gives the class its
public name; see [Type aliases and `export { X as Y }`](#type-aliases-and-export--x-as-y-).

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
   Under `--experimental-generic-mono`, primitive members spanning
   several categories expand as one alternative instead (see
   [primitive union arguments](#primitive-union-arguments)).
2. **Cross-overload dedup**: When multiple overloads expand to the same
   concrete parameter list, drop the duplicates. Two overloads that
   both truncate to `(callback)` produce only one binding.
3. **Suffix assignment**: Across all surviving expansions, compute
   `_with_X` / `_with_X_and_Y` suffixes that disambiguate them. The
   primary variant gets `""`; the others are named after the
   parameters that distinguish them.

   The primary is the first overload's shortest expansion, unless a
   shorter expansion matches its leading parameters (names and types),
   in which case that one is the primary. An empty expansion matches
   any prefix, so a zero-argument overload always takes the bare name.
   Declaration order alone would let a required-options overload claim
   the bare name for its longer form:

   ```ts
   get(key: string, options: Conditional): Promise<string | null>;
   get(key: string, options?: GetOptions): Promise<string>;
   put(key: string, value: string, options?: PutOptions): Promise<void>;
   put(entries: Record<string, string>, options?: PutOptions): Promise<void>;
   ```

   ```rust
   pub async fn get(this: &Store, key: &str) -> …;              // prefix of (key, options)
   pub async fn get_with_conditional(this: &Store, key: &str, options: &Conditional) -> …;
   pub async fn get_with_get_options(this: &Store, key: &str, options: &GetOptions) -> …;
   pub async fn put(this: &Store, key: &str, value: &str) -> …;  // first overload keeps it
   pub async fn put_with_entries(this: &Store, entries: &Object<JsString>) -> …;
   ```

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
| `string` / `"x"` | `&str`          | `_with_str`     |
| `number` / `1`  | `f64`            | `_with_f64`     |
| `boolean` / `true` | `bool`        | `_with_bool`    |
| `bigint`        | `BigInt`         | `_with_big_int` |
| `symbol`        | `&Symbol`        | `_with_symbol`  |
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

Passing `--experimental-generic-mono` selects wasm-bindgen's experimental
per-monomorphization path. Every generated extern block receives
`experimental_generic_mono`, type parameters omit the `JsGeneric` bound, and
bare type-parameter arguments are passed by value so each concrete Rust type
uses its native ABI representation:

```rust
#[wasm_bindgen(experimental_generic_mono)]
extern "C" {
    pub fn identity<T>(value: T) -> T;
}
```

Generated dictionary helpers call their extern setters from ordinary Rust, so
their type parameters carry the narrower ABI bound required at those call
sites. The extern declarations themselves remain unconstrained:

```rust
impl<T: ::wasm_bindgen::convert::IntoWasmAbi> EvaluationDetails<T> { /* … */ }
pub struct EvaluationDetailsBuilder<T> { /* … */ }
impl<T: ::wasm_bindgen::convert::IntoWasmAbi> EvaluationDetailsBuilder<T> { /* … */ }
```

The ABI bound stays on helper `impl` blocks whose methods call generated
setters. It is not attached to the builder struct itself, so merely storing or
returning a builder does not constrain its type parameter.

Array-buffer-view helper parameters retain their `TypedArray` widening bound.
In per-monomorphization mode the helper also repeats the reference ABI bound
that wasm-bindgen places on the generated import shim:

```rust
pub fn new<T: ::js_sys::TypedArray>(value: &T) -> TypedArrayOptions
where
    for<'__wbg> &'__wbg T: ::wasm_bindgen::convert::IntoWasmAbi,
{ /* … */ }
```

Concrete primitive arguments to locally declared generic types use native Rust
ABIs in this mode: `Details<boolean>`, `Details<number>`, and `Details<string>`
become `Details<bool>`, `Details<f64>`, and `Details<String>`. Aliases resolve
first, so `Details<Id>` with `type Id = string` is also `Details<String>`, and
a union with a single common type lowers as that type (`Details<"a" | "b">` is
`Details<String>`). [Enums](#string-and-numeric-enums) are native leaves too,
so they stay named (`Details<Mode>`, `Details<Level>`), and an awaited
`Promise<Mode>` resolves to `Mode`. A type parameter that shadows an alias
stays a type parameter. This native-leaf rule is deliberately limited to user
declarations. Built-in JS containers, tuples, iterators, and callbacks retain
their established wrapper elements (`Array<JsString>`,
`Map<JsString, JsString>`, and so on).

Direct string arguments use argument-position `impl JsStringLike`, allowing
`&str`, `String`, `&JsString`, and `JsString` without caller-side conversion.
Each occurrence is an independent anonymous type parameter, so different
string arguments may use different representations without generated names
that could shadow source declarations. String returns remain concrete and get
one additive zero-copy variant bound to the same JS name:

```rust
pub fn value(default_value: impl ::wasm_bindgen::JsStringLike) -> String;
#[wasm_bindgen(js_name = "value")]
pub fn value_js_string(default_value: impl ::wasm_bindgen::JsStringLike) -> JsString;
```

The return transform preserves nullable, fallible, async, and locally declared
generic wrappers, using the same alias and union resolution as generic
arguments. If a return has multiple eligible string leaves, one `_js_string`
method replaces all of them rather than generating a Cartesian set of
variants:

```rust
pub fn details() -> Pair<f64, String>;
#[wasm_bindgen(js_name = "details")]
pub fn details_js_string() -> Pair<f64, JsString>;
```

Getters, including static getters, get the same additive variant:

```rust
#[wasm_bindgen(method, getter, js_name = "flagKey")]
pub fn flag_key<T>(this: &EvaluationDetails<T>) -> String;
#[wasm_bindgen(method, getter, js_name = "flagKey")]
pub fn flag_key_js_string<T>(this: &EvaluationDetails<T>) -> JsString;
```

No variant is generated when the return has no string leaf outside built-in
containers (`Array<string>` keeps its single binding), or when a top-level
union becomes a [dynamic-union enum](#erased-return-position-unions-become-dynamic-union-enums):
`"a" | "b"` returns its enum only.

Async returns use native Rust values in this mode when the resolved type is
a primitive, a nullable primitive, or a locally declared generic type:

| TypeScript | Default | `--experimental-generic-mono` |
|---|---|---|
| `Promise<string>` | `Result<JsString, _>` | `Result<String, _>` + `_js_string` |
| `Promise<number>` | `Result<Number, _>` | `Result<f64, _>` |
| `Promise<boolean>` | `Result<Boolean, _>` | `Result<bool, _>` |
| `Promise<string \| undefined>` | `Result<JsOption<JsString>, _>` | `Result<Option<String>, _>` + `_js_string` |
| `Promise<Details<string>>` | `Result<Details<JsString>, _>` | `Result<Details<String>, _>` + `_js_string` |
| `Promise<Array<string>>` | `Result<Array<JsString>, _>` | unchanged |

This can increase code size because wasm-bindgen generates a separate shim and
descriptor for every instantiation. The option is experimental and tracks the
upstream attribute of the same name.

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

### Type-level generics propagate to every binding site

When the surrounding *type* declares parameters
(`interface Foo<T> { … }` / `declare class Foo<T> { … }`), those
parameters thread through every site that mentions `Foo`:

* The `pub type` declaration in the extern block —
  `pub type Foo<T: ::wasm_bindgen::JsGeneric>;`.
* Every getter / setter / method `fn` — redeclares
  `<T: ::wasm_bindgen::JsGeneric>` on the `fn` and uses
  `this: &Foo<T>`. The redeclaration is mandatory because each
  `extern "C"` `pub fn` is independent — the bound on `pub type`
  doesn't propagate to its sibling functions.
* For dictionaries: the factory `impl<T: ::wasm_bindgen::JsGeneric>
  Foo<T>` block, the wrapper struct
  `pub struct FooBuilder<T: ::wasm_bindgen::JsGeneric>`, and the
  builder `impl<T: ::wasm_bindgen::JsGeneric> FooBuilder<T>`.

Bounds are composed in a fixed order so the output is stable across
binding sites:

1. **Type-level params** (declared on `Foo<T, …>`), in declaration
   order. These come first so `<T: JsGeneric, U: JsGeneric>` aligns
   with `this: &Foo<T, U>`.
2. **Method-local params** collected from the callable's signature
   types (return type and parameters). A method's own
   `method<U>(…)` `U` lands here, behind any type-level params.
3. **ABV widening synthetics** (`<Tn: TypedArray>`) emitted by
   `generate_dictionary_params` for `ArrayBufferView` arguments.
   Always last.

`impl<T: …> Foo<T>` blocks (factories, builders) carry the type-level
bounds on the `impl` header; methods inside the impl do **not**
redeclare them (Rust forbids the duplicate). Only method-local
collected bounds and ABV synthetics appear on `fn`s inside an `impl`.

Return types are spelled out as `Foo<T>` / `FooBuilder<T>` rather than
`Self`, so the generic and non-generic factory paths emit the same
structural shape. The factory bodies use `Self::builder(…)` to
short-hand the static-method call (path position, not type position),
which works inside both kinds of `impl` blocks.

```ts
interface EvaluationDetails<T> {
  flagKey: string;
  value: T;
  variant?: string | undefined;
}
```

becomes

```rust
#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(extends = Object)]
    pub type EvaluationDetails<T: ::wasm_bindgen::JsGeneric>;
    #[wasm_bindgen(method, getter, js_name = "flagKey")]
    pub fn flag_key<T: ::wasm_bindgen::JsGeneric>(this: &EvaluationDetails<T>) -> String;
    #[wasm_bindgen(method, getter)]
    pub fn value<T: ::wasm_bindgen::JsGeneric>(this: &EvaluationDetails<T>) -> T;
    // … setters mirror the getters …
}
impl<T: ::wasm_bindgen::JsGeneric> EvaluationDetails<T> {
    pub fn new(flag_key: &str, value: &T) -> EvaluationDetails<T> {
        Self::builder(flag_key, value).build()
    }
    pub fn builder(flag_key: &str, value: &T) -> EvaluationDetailsBuilder<T> { … }
}
pub struct EvaluationDetailsBuilder<T: ::wasm_bindgen::JsGeneric> {
    inner: EvaluationDetails<T>,
}
impl<T: ::wasm_bindgen::JsGeneric> EvaluationDetailsBuilder<T> {
    pub fn variant(self, val: &str) -> Self { … }
    pub fn build(self) -> EvaluationDetails<T> { self.inner }
}
```

### Primitive union arguments

In per-monomorphization mode, primitive union arguments use `js_sys`'s
primitive-union marker traits instead of
[signature flattening](#signature-flattening). When a parameter's
alternatives include primitives from two or more of `bigint`, `boolean`,
`number`, and `string`, those alternatives merge into one argument-position
`impl` bound named `Js` + the categories in alphabetical order joined by
`Or` + `Like`:

```ts
function setValue(value: string | number): void;
function setLevel(level: 1 | 2 | "auto"): void;
function hasKey(key: PropertyKey): boolean;
function send(to: Target | string | number): void;
```

```rust
pub fn set_value(value: impl ::js_sys::JsNumberOrStringLike);
pub fn set_level(level: impl ::js_sys::JsNumberOrStringLike);
pub fn has_key(key: impl ::js_sys::PropertyKey) -> bool;
pub fn send(to: &Target);
#[wasm_bindgen(js_name = "send")]
pub fn send_with_number_or_string(to: impl ::js_sys::JsNumberOrStringLike);
```

* Literals count toward their category, so `1 | 2 | "auto"` is a
  `number | string` union.
* Aliases resolve first, and `null` / `undefined` arms are dropped as for
  every argument-position union.
* Non-primitive members keep their own overloads. The grouped binding takes
  the position of the first primitive member, and its `_with_` suffix lists
  the categories (`_with_number_or_string`).
* A union covering one category (`"a" | "b" | string`) is not a primitive
  union and keeps the `impl JsStringLike` / concrete-type rules above.
* `symbol` only groups as part of `number | string | symbol`, which maps to
  `js_sys::PropertyKey` whether it's spelled `PropertyKey` or written out.
  In any other mix, `symbol` members keep their own `&Symbol` overload
  and the remaining members group: `string | symbol | boolean` becomes
  `impl JsBooleanOrStringLike` plus `_with_symbol(&Symbol)`, and
  `string | symbol` stays `impl JsStringLike` plus `&Symbol`.
* Dictionary factories keep literal members as `new_<literal>` constructors
  and group only the remaining non-literal members.
* Setters group the members left after
  [literal union setters](#literal-union-setters) take their enum, so
  `level: "low" | "high" | number` is `set_level(LevelKind)` plus
  `set_level_with_f64(f64)`.

Return positions are unchanged: an erased union still becomes a
[dynamic-union enum](#erased-return-position-unions-become-dynamic-union-enums).
Without `--experimental-generic-mono` the traits aren't available, so
primitive unions (including `PropertyKey`) keep the per-member fan-out.

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

### Intersections take their most specific member

Parsing normalises every intersection: nested `&` is inlined, repeats
and the identity elements `unknown` and `object` are dropped, since
`A & object` is `A`, and `any` absorbs the whole intersection as in
TypeScript. An inline `{ … }` lowers to `object`, so where an
intersection is not hoisted to an interface (see
[Intersections](#intersections) under anonymous interface synthesis)
the common required-field refinement collapses to the named type, and
`(string & {})` inside a union is the plain `string` arm. A single
survivor is used bare; none is `Object`.

Two or more surviving members reach codegen as an `Intersection` and
are lowered to a bound. `A & B` is a subtype of every member, so each
member is a sound upper bound and the only question is which is most
specific: a named member whose ancestor chain contains all the others.
Unrelated or unnamed members erase to `JsValue` with a diagnostic:

```text
Options & { onlyIf: Conditional }    -> Options     (literal refines Options)
"a" | "b" | (string & {})            -> "a" | "b" | string
T & { id: string }                   -> T
Derived & Base                       -> Derived     (Derived extends Base)
Derived & any                        -> any
{ a: string } & { b: number }        -> Object
Derived & Unrelated                  -> JsValue     (no member subtypes the other)
```

In argument position the bound accepts slightly more than TypeScript
does; in return position the literal's extra fields are not reachable.
Neither is unsound at the ABI.

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
#[wasm_bindgen(private)]
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

wasm-bindgen enum discriminants are either all strings (optionally
beside tuple variants) or all 32-bit integers (unit variants only).
So a union made up entirely of distinct integer literals that fit `u32`
(or `i32`, when any is negative) synthesises a C-style enum directly.
Otherwise number literals first widen to `number` and boolean literals
to `boolean`, then three layered rules decide:

1. **A single remaining member** keeps its plain lowering:
   `true | false` → `bool`, `number | 0` and `0.5 | 1` → `f64`.
2. **Any string literal** ⇒ synthesise. Pure string-literal unions
   (`"a" | "b"`) become string-discriminant enums; mixed
   `"a" | string` becomes a dynamic union with literal variants + a
   fallback tuple, and `true | "x" | "y"` gets a `Bool(bool)` tuple.
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

* **Literal-discriminant variants** for string literal members, and
  integer literal members of a C-style enum. The variant name is
  PascalCased from the literal value (`N1`, `Neg1` for numbers); the
  discriminant is the literal itself, which wasm-bindgen routes as
  the literal value at the FFI boundary.
  ```rs
  pub enum RoleKind {
      User = "user",
      Assistant = "assistant",
      System = "system",
  }

  pub enum DirectionReturnKind {
      Neg1 = -1,
      N0 = 0,
      N1 = 1,
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
  | `symbol`          | `Symbol`        | `Symbol(Symbol)` |
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

A `declare module "<spec>" { ... }` block introduces a set of
declarations whose `module_context` is `Module("<spec>")`. They are
**resolved** at parse time so cross-module references work, but
whether they are **emitted** is governed by `--export` and
`--external`.

An input describes an environment: its globals and the host modules
it can import. The output mirrors it, with the globals at the top
level and each emitted module in its own `pub mod`, one segment per
`:` or `/` component of the specifier:

| Specifier | Rust module |
|---|---|
| `node:fs` | `node::fs` |
| `node:fs/promises` | `node::fs::promises` |
| `cloudflare:email` | `cloudflare::email` |
| `assets:*` | `assets` |

Components without an identifier character, such as `*`, are dropped.
Modules sharing a prefix, and a namespace with the same path, share
one `pub mod`, so `cloudflare::email` sits next to the members of a
global `namespace Cloudflare`. The `--lib-name` module is the
exception: it is the library the output binds, so its declarations,
the file's top-level exports and any `declare module` of that name,
are emitted at the top level.

A module-context declaration has exactly three possible states:

1. **Exported** — `<spec>` is in `--export`. The declaration is
   emitted in the module's `pub mod` with its
   `#[wasm_bindgen(module = "<spec>")]` attribute attached to the
   extern block. References from other emitted code are qualified
   relative to where they are emitted (`cloudflare::email::EmailMessage`
   from the top level, `super::super::Socket` from `cloudflare::sockets`).

2. **Externalised (suppressed)** — every type in `<spec>` resolves
   through `--external`. The declaration is not emitted; references
   route through the external map (`use ::other::email::Bar as Bar;`
   plus a bare-ident use site).

3. **Resolution-only** — `<spec>` is in the type universe but listed
   in neither `--export` nor `--external`. The declaration is not
   emitted. Any cross-reference from an emitted decl produces a
   codegen **error**:

   ```text
   error: type `Bar` from module "x:foo" is referenced by exported
     declarations but module "x:foo" is not in --export and not in
     --external. Add `--export x:foo` to emit it, or
     `--external x:foo=...` to point at a separately-generated crate.
   ```

   The reference itself still emits as a bare ident plus a
   `use JsValue as Bar;` alias so the output compiles — the error
   surfaces at the next ts-gen run, not at `cargo build`.

`--external` always wins over `--export`: a module in both is
externalised, not emitted.

For `Global` declarations (top-level `declare class`, `declare
function`, `declare const`, etc. in a script file) the rule is simpler:
emit when the decl's source file is in `--export`, otherwise skip.

A file with any top-level `import` or `export` is a module, and with
`--lib-name` every top-level declaration in it is `Module("<lib>")`,
with or without an `export` prefix. An unexported `interface Options`
or `declare var _Foo` is still exported by name through
`export { _Foo as Foo }`, or referred to by the exported declarations,
and either way it merges and resolves with them rather than being a
global the output never emits.

Qualification keys off the *resolved* declaration's `module_context`,
not the textual name, so a global `interface Foo` and a module-scoped
`class Foo` resolve independently — scope-chain resolution at the use
site picks the visible one. Each module body is a child scope of its
file, as in TypeScript: its declarations see the file's globals, but
are not visible to the file or to other modules, and repeated
`declare module "<spec>"` blocks in one file share a scope. Another
file or module reaches them with `import { Foo } from "<spec>"`, which
resolves against the module's scopes before looking for a file.

### Output surface: `--export` and `--input`

`--input` defines the *type universe* — every file gets parsed so
resolution sees the full set. `--export` defines the *output surface* —
what actually gets emitted as public Rust code. Two forms:

* **File path** (e.g. `--export types/email.d.ts`) — emit that file's
  **global-scope** declarations.
* **Module specifier** (e.g. `--export cloudflare:email`) — emit that
  module's declarations in its `pub mod`, attribute-attached.

When `--export` is omitted entirely, every `--input` file becomes an
implicit file export. **No modules are emitted by the implicit
default** — module emission is always opt-in via an explicit `--export
<spec>`. As soon as one `--export` is passed, the implicit default is
disabled and only what's listed contributes to the output. So

```sh
ts-gen --input types/email.d.ts --export cloudflare:email
```

emits ONLY the `cloudflare:email` module (as `cloudflare::email`), not the
file's other globals. To get both you list both:

```sh
ts-gen --input types/email.d.ts \
       --export types/email.d.ts \
       --export cloudflare:email
```

No wildcards in v1 — list specific specifiers and file paths.

### `--external <spec>=<path>` — redirect to a separate crate

`--external "cloudflare:email=::other::email"` tells ts-gen that
everything declared in `cloudflare:email` lives in the
`::other::email::*` Rust path (typically a separately-generated
bindings crate). When **every** type in the module is covered by the
external map, ts-gen drops the module's emission entirely;
cross-module references render with the mapped path (a `use
::other::email::Bar as Bar;` alias at the top of the file plus a bare
`Bar` at use sites).

Partial coverage (some types mapped, some not) emits a warning,
falling back to the unresolved type's regular emission rules.

### Worked example: email bindings

```ts
// types/email.d.ts
declare module "cloudflare:email" {
  export class EmailMessage { /* ... */ }
}

declare class ForwardableEmailMessage extends EmailMessage { /* ... */ }

interface SendEmail {
  send(message: EmailMessage): Promise<EmailSendResult>;
}
```

With `--export types/email.d.ts --export cloudflare:email`:

```rust
pub mod cloudflare {
    pub mod email {
        #[wasm_bindgen(module = "cloudflare:email")]
        extern "C" {
            pub type EmailMessage;
        }
    }
}
#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(extends = cloudflare::email::EmailMessage)]
    pub type ForwardableEmailMessage;
}
#[wasm_bindgen]
extern "C" {
    pub type SendEmail;
    pub fn send(this: &SendEmail, message: &cloudflare::email::EmailMessage)
        -> Promise<EmailSendResult>;
}
```

### `namespace` is unrelated to JS modules

```ts
namespace WebAssembly {
  class Module { ... }
}
```

emits a `pub mod web_assembly { ... }` (note: still `pub`) with
`#[wasm_bindgen(js_namespace = "WebAssembly")]` on each member.
Namespaces describe runtime *property access* (`WebAssembly.Module`),
not module specifiers — they're independent of the `--export`
filter. A nested namespace nests its `pub mod`, and its members carry the
full segment chain as an array:

```rust
#[wasm_bindgen(js_namespace = ["Outer", "Inner"])]
pub fn run();
```

The module starts with `use super::*;`, so members name `js_sys` types,
globals and the file's external aliases bare, like top-level items. A
name resolves from where it is written, so inside a namespace a sibling
wins over a same-named global, and an alias resolves to its target. A
reference to a namespace member from outside its `pub mod` is qualified
with the path to it, relative to where the reference is emitted:

```ts
interface Emitter {}
declare namespace Events {
  interface Emitter {}
  interface Options { emitter: Emitter }   // Events.Emitter
  namespace Inner {
    interface Handle { emitter: Emitter }  // Events.Emitter
  }
}
interface Stream extends Events.Emitter {}
```

```rust
pub mod events {
    use super::*;
    // Options::emitter → Emitter
    pub mod inner {
        use super::*;
        // Handle::emitter → super::Emitter
    }
}
#[wasm_bindgen(extends = events::Emitter, extends = Object)]
pub type Stream;
```

A namespace inside a module takes the same shape, with the members
carrying `module = "…"` as well as `js_namespace`. This is what binds
value members correctly: in `@types/node`, `util.inspect.defaultOptions`
is a property of the exported function `inspect`, not an export of
`"util"`, so it needs both:

```ts
declare module "util" {
  export function inspect(object: any): string;
  export namespace inspect {
    let defaultOptions: InspectOptions;
  }
}
```

```rust
pub mod util {
    #[wasm_bindgen(module = "util")]
    extern "C" {
        pub fn inspect(object: &JsValue) -> String;
    }
    pub mod inspect {
        #[wasm_bindgen(module = "util")]
        extern "C" {
            #[wasm_bindgen(thread_local_v2, js_name = "defaultOptions", js_namespace = "inspect")]
            pub static default_options: super::InspectOptions;
        }
    }
}
```

`pub fn inspect` and `pub mod inspect` coexist, as Rust keeps values and
modules in separate namespaces. Qualified references (`Codes.Region`,
`Outer.Inner.Deep`, or `Inner.Deep` from within `Outer`) resolve through
the namespace and are emitted relative to the use site
(`codes::Region`, `super::inspect::Style`), keeping any generic
arguments (`Inner.Settings<number>` → `inner::Settings<Number>`). A
member that shares its name with a top-level type is a different
declaration with its own arity. A qualified name that resolves to
nothing erases to `JsValue` with an error rather than matching a local
type by its last segment.

A namespace the module assigns with `export =` is the module itself, so
its members are the module's exports and are emitted directly in the
module's `pub mod`, without `js_namespace`. In `node:assert`
(`function assert` + `namespace assert` + `export = assert`),
`strictEqual` binds as `import { strictEqual }`, since the module has
no `assert` export to reach it through.

The assigned binding itself, a function, class or variable, is the
default export of the module's ESM shape. It keeps its local Rust name
and binds with `js_name = "default"`:

```rust
pub mod node {
    pub mod assert {
        #[wasm_bindgen(module = "node:assert")]
        extern "C" {
            #[wasm_bindgen(js_name = "default")]
            pub fn assert(value: &JsValue);
        }
    }
}
```

Binding the namespace members as named exports assumes the module
exposes them that way, which Node builtins and packages with an ESM
facade do. A plain CommonJS `module.exports = f; f.x = …` exposes only
the default, since `cjs-module-lexer` does not detect `f.x`.

A `--lib-name` module file's top-level `export =` is handled the same
way. Only an `export =` of a binding declared in the same module is
handled; one naming a global is skipped with a diagnostic.

A namespace merged with a same-named class, function or interface
(`class Foo {}` + `namespace Foo { interface Options {} }`) leaves
that declaration as the type `Foo`, while `Foo.Options` resolves
through the namespace, in either declaration order.

## Type aliases and `export { X as Y }`

* `type Foo = Bar;` → `pub type Foo = Bar;` if `Bar` is a recognised
  type, or chases the alias chain to its terminal during codegen. An
  alias inside a namespace is emitted in the namespace's `pub mod`.
  A plain reference to an alias lowers to its target, so only a generic
  instantiation names the alias, qualified like any namespace member:

  ```ts
  declare namespace Events {
    type Name = string;
    type Listener<E> = (event: E) => void;
  }
  declare function onEvent(name: Events.Name, listener: Events.Listener<string>): void;
  ```

  ```rust
  pub mod events {
      pub type Name = JsString;
      pub type Listener<E> = Function<fn(E) -> Undefined>;
  }
  pub fn on_event(name: &str, listener: &events::Listener<JsString>);
  ```
* `export { Local as Public };` (sourceless) → names the binding `Local`
  declares in the exporting scope, recorded against its `TypeId` in
  `TypeRegistry::export_names`. Each `declare module "x"` body is its own
  scope, so the same local name in another module or at global scope is a
  different binding and keeps its own name. The public names are what the
  module exports, so value bindings import by them:
  * Functions and variables are emitted once per public name, with no
    alias stub: `function _connect(...); export { _connect as connect };`
    emits `pub fn connect(...)` bound to the JS export `connect`.
  * A constructor variable on its own is a value nothing refers to by its
    local name, so its class takes the first public name; any further
    names alias it.
  * A class keeps its local name as the Rust type, since the module refers
    to it by that name, but binds `js_name` to the first public name, and
    its constructors and static members carry `js_class` with the same
    name. Each public name gets an alias stub (`pub type Server = _Server;`).
    A constructor variable with an instance interface of the same name
    (`interface _Foo {…}; var _Foo: { new(): _Foo }`, or `var _Foo:
    _FooConstructor`) is a class in this sense: the interface's members
    refer to `_Foo`, so the merged class is `_Foo` with `js_name = "Foo"`
    and `pub type Foo = _Foo;`.
  * Types without a runtime binding (interfaces, type aliases, enums) get
    an alias stub only.

  A `Local` declared outside the exporting scope is left alone and only
  aliased.
* `export { X as Y } from "...";` (with source) → registered as an import
  from the named module.

## String and numeric enums

```ts
enum Color { Red = "red", Green = "green" }
type Kind = "text" | "bytes";
```

Both a string `enum` and a type alias over string literals emit a
wasm-bindgen string enum, whose variant discriminants are the JS
string values:

```rust
#[wasm_bindgen(private)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Color {
    Red = "red",
    Green = "green",
}
```

Numeric enums lower similarly with explicit discriminant values and a
`#[repr(u32)]` / `#[repr(i32)]` chosen from the value range.

### Member aliases and constant initialisers

Member initialisers are evaluated as TypeScript does: numeric and string
literals, unary and binary constant expressions with JS numeric
semantics (bitwise operators on int32, `>>>` on uint32), string
concatenation, and references to earlier members, bare or qualified by
the enum's own name. A member without an initialiser continues from the
previous member's evaluated value. An enum is numeric when any member
evaluates to a number, so `Read = 1 << 0` alone is enough, and a string
enum otherwise.

A member whose value can't be evaluated, such as `B = Other.X`, or one
continuing from it, is skipped with a diagnostic. Its value is only known
at runtime, and a guessed discriminant could repeat a real member's value
and turn that member into an alias of it.

A member whose value repeats an earlier member's is an alias, not a
second variant: wasm-bindgen rejects duplicate discriminants, and two
string variants with one value could not be told apart when decoding.
The first member with a value is the variant; later ones become
associated consts of it, so `Version::Latest == Version::V2` holds and
both round-trip identically:

```ts
declare enum Version { V1 = 1, V2 = 2, Latest = 2, Current = V2 }
declare enum Flags { Read = 1 << 0, Write = 1 << 1, ReadWrite = Read | Write }
declare enum Kind { Text = "text", Bytes = "bytes", Default = "text" }
```

```rust
#[repr(u32)]
pub enum Version { V1 = 1, V2 = 2 }
impl Version {
    #[allow(non_upper_case_globals)]
    pub const Latest: Version = Version::V2;
    #[allow(non_upper_case_globals)]
    pub const Current: Version = Version::V2;
}
#[repr(u32)]
pub enum Flags { Read = 1, Write = 2, ReadWrite = 3 }
pub enum Kind { Text = "text", Bytes = "bytes" }
impl Kind {
    #[allow(non_upper_case_globals)]
    pub const Default: Kind = Kind::Text;
}
```

wasm-bindgen converts both kinds by value only, so unlike other named
types they are not borrowed in argument position: `fn send(kind: Kind)`,
`fn set_kind(this: &Opts, val: Kind)`, and optional properties still
read back as `Option<Kind>`. This holds through aliases and qualified
names (`NS.Kind`).

Like primitives they cross the ABI natively rather than as a JS value, so
they have no `JsGeneric` or slice form. An enum follows the primitive it
carries wherever that primitive would not stay native: inside a
`JsGeneric` position (`js_sys` containers, callbacks, erased local
generics) it takes the primitive's wrapper, and in a slice the primitive
itself:

| TypeScript             | Rust                              |
| ---------------------- | --------------------------------- |
| `kinds: Kind[]`        | `&[String]` / `Vec<String>`       |
| `levels: Level[]`      | `&[f64]` / `Vec<f64>`             |
| `Promise<Kind>`        | `Promise<JsString>`               |
| `Array<Level>`         | `Array<Number>`                   |
| `(kind: Kind) => void` | `Function<fn(JsString) -> Undefined>` |
| `Holder<Kind>`         | `Holder<JsString>`                |

In [per-monomorphization mode](#in-scope-generic-type-parameters) a local
generic argument and an awaited `Promise` are native positions, so there
an enum stays named exactly as `string` stays `String`: `Holder<Kind>`,
`Details<Option<Level>>`, `async fn next() -> Result<Kind, JsValue>`.
Slices and `js_sys` containers are unchanged by the mode.

`Kind::from_js_value(&value)` converts back. A
[dynamic union](#erased-return-position-unions-become-dynamic-union-enums)
carries an enum as a payload, `Kind | number` → `Kind(Kind)`, `F64(f64)`,
dispatched through the enum's `TryFromJsValue`.

Generated enums describe values on the JS side, not anything the crate
exports, so every string enum, numeric enum and dynamic union is
`#[wasm_bindgen(private)]`: wasm-bindgen keeps it off the JS module's
exports and lets same-named private copies coexist in one build. An enum
inside a namespace carries the namespace's `js_namespace`, like its
imports:

```rust
pub mod timing {
    #[wasm_bindgen(private, js_namespace = "Timing")]
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum DurationUnit { /* ... */ }
}
```

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
   `DOMException`, `ReadableStream`, `Response`, `URL`, `URLSearchParams`,
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
generated file's `use js_sys::*;` prelude. The one `js_sys` class whose
Rust spelling differs from TS, `URIError`, is aliased from
`::js_sys::UriError` through the same default-mapping path
(`external_map::JS_SYS_RENAMES`) and likewise survives `--no-web-sys`.
