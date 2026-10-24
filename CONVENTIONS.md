# Conventions: TypeScript → Rust

This document is the canonical reference for how `ts-gen` translates
TypeScript declarations into `wasm-bindgen` Rust bindings. It covers the
patterns we handle, what they emit, and the rules behind each translation.

Conventions are listed roughly from simplest to most complex. New
conventions belong here first; tests and codegen come second. Keep the file
in sync with the snapshot fixtures (`tests/fixtures/*.d.ts` paired with
`tests/snapshots/<name>/{default,mono}.rs`).

> **Maintenance**: when changing a convention or adding a new one, update
> this file in the same PR. Diff-only snapshot changes that aren't
> documented are a smell.

## Contents

* [Primitive types](#primitive-types)
* [Optional and nullable types](#optional-and-nullable-types)
  * [`--strict-nullable` — exact read carriers](#--strict-nullable--exact-read-carriers)
* [Array and slice types](#array-and-slice-types)
* [Function types](#function-types)
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
  * [`--fallible` — single `Result` form](#--fallible--single-result-form)
  * [`never` returns keep only the catching form](#never-returns-keep-only-the-catching-form)
  * [`toString()` is `Display`](#tostring-is-display)
* [`Promise<T>` returns become `async fn`](#promiset-returns-become-async-fn)
* [`@throws` JSDoc → typed error](#throws-jsdoc--typed-error)
* [`@rust` JSDoc → Rust scalar](#rust-jsdoc--rust-scalar)
* [`--annotate` side files](#--annotate-side-files)
* [Subtyping LUB across unions](#subtyping-lub-across-unions)
* [Module declarations and namespace nesting](#module-declarations-and-namespace-nesting)
* [Type aliases and `export { X as Y }`](#type-aliases-and-export--x-as-y-)
* [String and numeric enums](#string-and-numeric-enums)
* [Type-level evaluation](#type-level-evaluation)
* [Multiple-context name resolution](#multiple-context-name-resolution)
* [Duplicate identifiers](#duplicate-identifiers)
* [External type mapping and web platform defaults](#external-type-mapping-and-web-platform-defaults)
* [Provided functions (`--provides`)](#provided-functions---provides)
* [Wasm ABI (`--target wat` / `embed`)](#wasm-abi---target-wat--embed)

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

`never` is dropped from a union, so a merged `value: T` /
`value?: never` property reads as `Option<T>` and is set with `&T` (see
[Optional and nullable types](#optional-and-nullable-types)). A callable
returning `never` binds only its catching form, typed `()` (see
[`never` returns](#never-returns-keep-only-the-catching-form)).

TypeScript's built-in `PropertyKey` is recognised without a declaration
and lowers exactly like `string | number | symbol`.

## Optional and nullable types

`null` and `undefined` are ordinary union members in the IR, as in
TypeScript. Every union is built through one constructor that keeps it
in a canonical form: nested unions are flattened (a read also resolves
the union aliases it names, [to their members](#union-aliases-flatten-into-the-read)),
repeats are dropped, `void`
inside a union is `undefined`, value members keep their source order
and the empties come last, `null` before `undefined`. A union of one
value and an empty is still a union, so `T | null`, JSDoc `?T` and
`T?` are all `T | null`, and `null | undefined` is a union of the two
empties. Optionality is a property of the declaration, not the type:
`x?: T` and `f(x?: T)` record `optional` beside a plain `T`, and a
parameter with a default initializer (`f(x = 1)`) is optional too.

Lowering reads the empties off a union (merging empties held by
aliases, so `MaybeStr | undefined` with `type MaybeStr = string | null`
admits both, as does a qualified `Intl.LocalesArgument` carrying its own
`undefined`) and picks a carrier for each top-level position. The
direction decides the carrier. A write has to choose what an empty
crosses as: `IntoWasmAbi` sends `None` as `undefined`, so `Nullable<T>`
carries `null`, and the two carriers, each owning one empty, nest for the
union. A read chooses nothing: `FromWasmAbi` receives whatever JS sent,
and `Option<T>` reads `null` and `undefined` alike as `None`, so every
empty-admitting read is `Option<T>` and loses nothing by it. A type
argument has no direction and names the union as written.

| TS                       | Write / type argument | Read        | JS → Rust                                     | Rust → JS             |
| ------------------------ | --------------------- | ----------- | --------------------------------------------- | --------------------- |
| `T \| undefined`         | `Option<T>`           | `Option<T>` | `undefined` is `None`                         | `None` is `undefined` |
| `T \| null`              | `Nullable<T>`         | `Option<T>` | `null` is `Null` written, `None` read         | `Null` is `null`      |
| `T \| null \| undefined` | `Option<Nullable<T>>` | `Option<T>` | `undefined` is `None`, `null` is `Some(Null)` written, `None` read | each state as itself |

`Nullable<T>` is wasm-bindgen's two-state `enum Nullable<T> { Null,
Value(T) }` (in the prelude, and re-exported as `js_sys::Nullable`), so a
Node-style `fn(x: T | null)` that checks `x === null`, or a
default-parameter initializer that fires on `undefined`, receives exactly
the empty passed. The `Option` layer over a `Nullable` is always strict,
so `null` reaches the `Nullable`; a bare `Option<T>` reads `null` as
`None` under either cfg (wasm-bindgen#5428) while its `None` still
crosses as `undefined`, which is why it is the read carrier and not the
write one. wasm-bindgen has `From<Option<T>> for Nullable<T>` and
`From<Nullable<T>> for Option<T>`, so a value read as `Option<T>` is
written back through a `Nullable<T>` argument with `.into()`, and
`opt.and_then(Nullable::value)` collapses a written union to `Option<T>`.
`Nullable<Nullable<T>>`, `Nullable<Option<T>>` and `Option<Option<T>>`
are never emitted.

### Reads

Getters, static getters, function and method returns (the `Ok` of a
`catch` `Result` included), the awaited value of an async function and
a [named union alias](#named-union-aliases) read whole are reads:
`T | null`, `T | undefined` and `T | null | undefined` are all
`Option<T>`. A read admits `undefined` when its type does or when its
declaration is optional, so `x?: T` and `x?: T | null` both read as
`Option<T>`, for instance and static properties alike. `null |
undefined` with nothing else reads as `Option<Null>`. A read whose type
erases to `JsValue` (an unrelated intersection, a union with no useful
bound, an unresolved name) takes the same `Option<JsValue>` when it
admits an empty, so an absent field or a stored `null` reads as `None`
there too. The top type is the one exception: `any` and `unknown`
already admit `undefined` and `null` in-band (`is_undefined`,
`is_null`), TypeScript itself erases a `| null` or `| undefined` on
them, and an `Option` would only fold what the `JsValue` already tells
apart, so `x?: any`, `any | null` and `unknown | null | undefined` stay
a bare `JsValue`.

| TS                                  | Rust read                                       |
| ----------------------------------- | ----------------------------------------------- |
| `get(key): Promise<R2ObjectBody \| null>` | `async fn get(..) -> Result<Option<R2ObjectBody>, JsValue>` |
| `get(key): Promise<string \| null>` | `async fn get(..) -> Result<Option<String>, JsValue>` |
| `f(): Promise<number \| undefined>` | `async fn f() -> Result<Option<f64>, JsValue>`  |
| `f<T>(): Promise<T \| null>`        | `async fn f<T>() -> Result<Option<T>, JsValue>` |
| `lookup(): string \| null \| undefined` | `fn lookup() -> Option<String>`            |
| `readonly x?: string \| null`       | `fn x(&self) -> Option<String>`                 |
| `x?: A & B` (erased)                | `fn x(&self) -> Option<JsValue>`                |
| `x: (A & B) \| null` (erased)       | `fn x(&self) -> Option<JsValue>`                |
| `x?: any`, `x?: unknown \| null`    | `fn x(&self) -> JsValue`                        |
| `deserializeAttachment(): any \| null` | `fn deserialize_attachment(&self) -> JsValue` |

The awaited value [lowers like a sync return](#awaited-values-lower-like-sync-returns),
so the carrier holds the read form (`String`, `f64`, an enum), not the
`JsString` / `Number` handle. An awaited carrier around a type
parameter is bounded whole (`Option<T>: FromWasmAbi`; a native such as
`f64` has an `Option` ABI of its own without being `OptionFromWasmAbi`),
and under `--strict-nullable` a `Nullable<T>` read bounds
`T: NullableFromWasmAbi`, its only impl. Type arguments have no direction
and keep the exact carrier (`Map<String, Nullable<String>>`,
`Holder<Nullable<String>>`, the `Function<fn(..)>` signature slots), as
does a nullable alias declaration (`type MaybeStr = string | null` is
`Nullable<String>` under per-monomorphization, and a `Holder<MaybeStr>`
reads its `T` as such).

### Writes

Function, method and constructor arguments, setters, builder methods and
factories expand the union instead: each empty is an arm.

* `null` gets a variant of its own with no argument for that position:
  `f(x: T | null)` gives `f(x: &T)` and `f_with_null()`. A variant
  passing only `null` never takes the bare name, which would read as
  omitting the argument: `f(x: null)` gives `f_with_null()` alone, and
  `f(a: null, b: null)` `f_with_a_null_and_b_null()`.
* `undefined` is absence. A parameter is omittable when it is optional,
  or when it is trailing and its type admits `undefined` (so a required
  trailing `x: T | undefined` expands like `x?: T`). Trailing-ness
  follows the omission chain: a parameter is trailing when every
  parameter after it can be omitted too. An omittable parameter drops
  `undefined` from its value arms, which take the plain type; one that
  cannot be omitted keeps `Option<T>`.
* A setter or builder method expresses `undefined` by not being called,
  so `x?: T`, `x?: T | undefined` and `x: T | undefined` all give
  `set_x(&T)`. A setter whose only arm is `null` is `set_x_with_null()`,
  by the rule above.

| TS                                  | Write variants                                                       |
| ----------------------------------- | -------------------------------------------------------------------- |
| `f(x: T \| null)`                   | `f(x: &T)`, `f_with_null()`                                          |
| `f(x: T \| undefined)`              | `f()`, `f_with_x(x: &T)`                                             |
| `f(x?: T \| null)`                  | `f()`, `f_with_x(x: &T)`, `f_with_null()`                            |
| `f(x: string \| number \| null \| undefined)` | `f()`, `f_with_str(x: &str)`, `f_with_f64(x: f64)`, `f_with_null()` |
| `f(a: T \| undefined, b: number)`   | `f(a: Option<&T>, b: f64)`                                           |
| `f(a: T \| undefined, b?: number)`  | `f()`, `f_with_a(a: &T)`, `f_with_a_and_b(a: &T, b: f64)`            |
| `f(a?: T, b?: U \| null, c?: V)`    | `f()`, `f_with_a`, `f_with_a_and_b`, `f_with_a_and_null`, `f_with_a_and_b_and_c`, `f_with_a_and_null_and_c` |
| `x?: T \| null` (setter)            | `set_x(&T)`, `set_x_with_null()`                                     |
| `x?: T \| null` (builder)           | `.x(&T)`, `.x_with_null()`                                           |
| `f(x: null)`                        | `f_with_null()`                                                      |
| `x?: null` (setter)                 | `set_x_with_null()`                                                  |

* **Argument position**: after the `null` arm is split off, an
  `undefined` that cannot be omitted is `Option<&T>` around the borrowed
  form (`Option<f64>`, `Option<ModeKind>` for an enum, `Option<&[f64]>`
  keeping its slice mode). When `T` is a type parameter it is taken by
  value (`Option<T>`): the macro cannot borrow a generic. A `JsValue`
  inner (`any | null`) collapses to `&JsValue` and has no `null` arm, as
  `JsValue::NULL` is the empty. Union inners fan out as usual and the
  `null` arm sits beside them (`new_with_str(body: &str)`,
  `new_with_blob(body: &Blob)`, `new_with_null()`), so
  `Response(body?: BodyInit | null)` gives `new()`, `new_with_str`, ...
  and `new_with_null()`.
* **The `null` arm** binds the import privately as `__<name>` taking
  `&JsValue` in that position (with `js_name` naming the JS member), and
  the public `<name>` wraps it, passing `&JsValue::NULL`. A type
  parameter used only by the `null` position is dropped from the
  wrapper. The arm is `_with_null` when one parameter admits `null`; with
  more, each is named for its parameter (`new_with_body_null`,
  `new_with_body_null_and_length(length)`, `new_with_length_null(body)`).
* **Properties keep TypeScript's distinction** between `x?: T` and a
  required `x: T | undefined`. The first is set through the builder or
  setter; the second is a required factory argument taking
  `Option<&T>`, whose factory calls `set_x(val)` for `Some` and leaves
  the property unset for `None`. A required `x: null | undefined` takes
  `Option<&Null>` and calls `set_x_with_null()` for `Some`.
* A discriminated-union branch whose property is required in one
  branch and optional in another has a merged setter of the optional
  shape; the branch factory keeps the branch's own type (a `null` branch
  calls `set_x_with_null()`; see
  [Per-branch required fields](#per-branch-required-fields-in-discriminated-unions)).

### Inner positions

A position inside a generic container (a `Promise<T | null>` argument
or [`_promise` form](#name_promise--the-un-awaited-form) return,
`Array<T | undefined>`, a callback's arguments and return value) cannot
expand, and needs a `JsGeneric` handle: `JsNullable<T>` when `null` is
admitted (`T | null | undefined` too; `is_empty` covers both),
`JsOption<T>` for `T | undefined` only. A `JsValue` inner stays bare.
A nullable alias declaration (`type MaybeStr = string | null`) is
`JsNullable<JsString>` for the same reason.

### Per-monomorphization

`experimental_generic_mono` expands writes identically, with the same
names; a string argument takes `impl JsStringLike`
(`Option<impl JsStringLike>` where it cannot be omitted). Inner
positions use the carriers where wasm-bindgen accepts them (a returned
`Promise<T>` is awaited through `JsFuture<T: FromWasmAbi>`, so its `T`
takes the read carrier; a user generic's argument is a type argument and
keeps the exact one):

| Position                                    | Mono                                  |
| ------------------------------------------- | ------------------------------------- |
| returned `Promise<string \| null>`          | `Promise<Option<JsString>>`           |
| user generic `Holder<string \| null>`       | `Holder<Nullable<String>>`            |
| `type MaybeStr = string \| null`            | `pub type MaybeStr = Nullable<String>` |
| `Promise<T \| null>` argument               | `&Promise<JsNullable<T>>`             |
| `T[]`, `Array<..>`, `Record<..>`, `Map<..>` | `JsNullable<T>` / `JsOption<T>`       |
| `Function<fn(..)>` arguments and return     | `JsNullable<T>` / `JsOption<T>`       |

The remaining handle positions are ones wasm-bindgen does not accept a
carrier in yet: `Function<fn(..)>` needs `ErasableGeneric<Repr =
JsValue>` and `JsCast` on its signature types, slices need
`VectorRefIntoWasmAbi`, and the js-sys `Array`, `Map` and `Object`
APIs (and `Promise::resolve`) need `ErasableGeneric<Repr = JsValue>` on
the element.

### `--strict-nullable` — exact read carriers

`--strict-nullable` (`GenerateOptions::strict_nullable(true)`) gives
reads the write column of [the carrier table](#optional-and-nullable-types):
`T | null` reads as `Nullable<T>`, `T | undefined` as `Option<T>` and
`T | null | undefined` as `Option<Nullable<T>>`, so a caller that must
tell a received `null` from a received `undefined` (a Node-style
`lookup(): string | null | undefined` returning `null` for an unknown
key and `undefined` for a known empty one) gets each state as itself,
in both modes. A read erasing to `JsValue` stays bare under the flag,
except a property that can be absent, which keeps its `Option` so
absence reads as `None`, with a written `null` as the `Nullable` under
it (`x?: (A & B) | null` is `Option<Nullable<JsValue>>`); the top type
stays bare as always. Writes, type arguments and inner positions are the
same under either setting, as is every name.

| TS                                        | Default                  | `--strict-nullable`              |
| ----------------------------------------- | ------------------------ | -------------------------------- |
| `get(): string \| null`                   | `Option<String>`         | `Nullable<String>`               |
| `lookup(): string \| null \| undefined`   | `Option<String>`         | `Option<Nullable<String>>`       |
| `readonly x?: string \| null`             | `Option<String>`         | `Option<Nullable<String>>`       |
| `f(): Promise<T \| null>` (awaited)       | `Result<Option<T>, _>`   | `Result<Nullable<T>, _>`         |
| `x: (A & B) \| null` (erased)             | `Option<JsValue>`        | `JsValue`                        |
| `f(x: string \| null)` (write)            | `f(&str)`, `f_with_null()` | the same                       |

The `nullability`, `node-nullable`, `optional-reads` and
`promise-companion` fixtures carry a `strict.rs` snapshot beside their
`default.rs` and `mono.rs`.

## Array and slice types

`T[]`, `Array<T>` and `ReadonlyArray<T>` are the same TypeScript type
and lower identically, to a Rust-idiomatic sequence at the top level;
`ArrayLike<T>` is the distinct TS type that stays the `Array<U>`
handle, for callers who want to pass a JS array reference with no
element-wise materialisation (a bare `Array` does too):

* **Argument position** → `&[T]`, which JS receives as a plain `Array`
  (see [slice modes](#slice-modes)).
* **Return position** → `Vec<T>`.

wasm-bindgen has both a slice and a `Vec` ABI for Rust-native `String`,
`f64` and `bool` elements and for JS handles. So an element that
resolves, through aliases, unions and [enums](#string-and-numeric-enums),
to a string, number or boolean keeps its return-position form; every
other element takes its inner-position handle form:

| TypeScript element           | Slice / Vec element    |
| ---------------------------- | ---------------------- |
| `number`, `1 \| 2`           | `f64`                  |
| `string`, `"a" \| "b"`       | `String`               |
| `Foo` (named JS / Rust type) | `Foo`                  |
| `any` / `unknown`            | `JsValue`              |
| `boolean`                    | `bool`                 |
| `T \| null`                  | `JsNullable<U>`        |
| `T[]`                        | `Array<U>`             |

Strings stay owned (`Vec<String>`, not `Vec<&str>`); JS-imported
classes stay unborrowed (`&[EmailAttachment]`, not
`&[&EmailAttachment]`); numbers stay bare (`&[f64]`, not
`&[Number]`). With `type Row = string[]`, `Row[]` is
`&[Array<JsString>]` / `Vec<Array<JsString>>`, the same as
`string[][]`.

Inside a built-in container (`Promise<Array<T>>`, `Map<K, Array<V>>`,
…) the element stays the `Array<T'>` handle, which type-checks against
both the stable js-sys API and `js_sys_unstable_apis`, where container
elements are real generics.

### Slice modes

wasm-bindgen's `#[wasm_bindgen(slice = view | array | copy)]` selects
how a numeric-element slice argument reaches JS: a typed-array
**view** of wasm linear memory (the default — zero-copy, but it
dangles if the callee retains it and detaches if memory grows during
the call), a fresh plain **`Array`**, or a fresh typed-array **copy**
the callee owns.

ts-gen never emits the view: a generated binding cannot assert that a
callee neither retains the buffer nor re-enters wasm. The TS type picks
the copy shape instead:

* A `T[]` parameter is a plain JS `Array` the callee may keep, so a
  slice of numbers (`&[f64]` or `Option<&[f64]>`)
  gets `slice = "array"`, after resolving aliases, literal unions and
  numeric enums.
* Under [per-monomorphization](#in-scope-generic-type-parameters), a
  `T[]` whose element is a type parameter gets `slice = "array"` too:
  wasm-bindgen resolves the mode per instantiation, so `T = f64` reaches
  JS as an `Array` rather than a view, and `array` fits every
  non-numeric `T`, which reaches JS as a fresh `Array` anyway.
* A typed slice from a [typed-array union](#typed-array-unions) gets
  `slice = "copy"`.

```rs
pub fn set_weights(#[wasm_bindgen(slice = "array")] weights: &[f64]);
pub fn generic<T>(#[wasm_bindgen(slice = "array")] values: &[T]) -> String; // per-mono
```

Every other slice takes no mode, as the modes only act on numeric
elements and wasm-bindgen already hands JS a fresh `Array`: JS handles
(`&[WebSocket]`, `&[JsString]`) are read in place, and Rust values
(`&[String]`, `&[bool]`, string enums) are decoded element by element.
Variadic rest parameters don't take one either: JS spreads the elements
into the call, copying them out of a numeric view before the callee runs,
so nothing can retain the view or re-enter wasm while it is live.

The user-facing Rust signature is unchanged — only the JS-side wire
format changes. The mode goes on the argument rather than the
function, so a typed copy sharing a signature with a `T[]` stays a
copy, and no inherited mode reaches a parameter it can't act on.

### `{@slice}` JSDoc hints

A `{@slice view|array|copy}` inline tag in a `@param` description (or
in a property's JSDoc, for its setter value) selects the mode for that
parameter, overriding the inferred one where the lowering is
slice-shaped:

```ts
/**
 * @param data {@slice view} the queue copies the data before returning
 */
writeBuffer(offset: number, data: Uint8Array | number[]): void;
```

```rs
pub fn write_buffer(this: &GPUQueue, offset: f64, #[wasm_bindgen(slice = "view")] data: &[u8]);
```

`view` is the author asserting, as a fact about the host API, that the
callee neither retains the buffer past the call nor re-enters wasm —
ts-gen never infers it, and emits it explicitly rather than as the
bare default so the assertion is greppable in generated code.

On a bare typed array, `copy` or `view` lowers the parameter to the
element slice (`&[u8]`), the "Rust callers hold native bytes here"
form, with the typed array kept as a `_with_` handle overload — the
same shape a `TypedArray | number[]` union produces.

A typed array with an `ArrayBuffer` argument (`Uint8Array<ArrayBuffer>`)
is TS 5.7+'s spelling of a WebIDL buffer without `[AllowShared]`: the
callee rejects a view of a `SharedArrayBuffer`. An atomics build's wasm
memory is shared, so a `view` there is emitted twice, a view for builds
without `target_feature = "atomics"` and a fresh copy for atomics builds.
The bare form defaults to `ArrayBufferLike`, which accepts shared memory,
so its view stays as is.

```ts
/** @param data {@slice view} */
digest(data: Uint8Array<ArrayBuffer>): void;
```

```rs
#[cfg(not(target_feature = "atomics"))]
pub fn digest(#[wasm_bindgen(slice = "view")] data: &[u8]);
#[cfg(target_feature = "atomics")]
pub fn digest(#[wasm_bindgen(slice = "copy")] data: &[u8]);
pub fn digest_with_uint8_array(data: &Uint8Array);
```

The split is on the whole function because wasm-bindgen doesn't evaluate
a `cfg_attr` on a parameter.

The tag is stripped from the rendered doc comment. A hint whose
parameter's lowering isn't slice-shaped is dropped by shape, the same
rule wasm-bindgen applies to an inherited mode (which is what lets one
hint cover a union whose typed-array handle overload isn't a slice).
An unknown mode or parameter name warns and is ignored.

### Typed-array unions

`TypedArray | number[]` (or `bigint[]` with `BigInt64Array` /
`BigUint64Array`) is how wasm-bindgen declares a numeric slice
argument, and how libraries accept either form. The array alternative
takes the typed array's element and comes first, tagged
`#[wasm_bindgen(slice = "copy")]` so JS receives a fresh typed array it
owns rather than a view of linear memory (which would dangle if the
callee retained it or memory grew during the call); the typed array
keeps a handle overload.

```ts
function take(values: Uint8Array | number[]): void;
function take64(values: BigInt64Array | bigint[]): void;
```

```rs
pub fn take(#[wasm_bindgen(slice = "copy")] values: &[u8]); // a fresh `Uint8Array`
pub fn take_with_uint8_array(values: &Uint8Array);
pub fn take64(#[wasm_bindgen(slice = "copy")] values: &[i64]); // a fresh `BigInt64Array`
pub fn take64_with_big_int64_array(values: &BigInt64Array);
```

`Uint8ClampedArray | number[]` pairs the same way, with the slice in
the `wasm_bindgen::Clamped` wrapper that binds a `Uint8ClampedArray`:

```ts
function fill(data: Uint8ClampedArray | number[]): void;
```

```rs
pub fn fill(#[wasm_bindgen(slice = "copy")] data: ::wasm_bindgen::Clamped<&[u8]>);
pub fn fill_with_uint8_clamped_array(data: &Uint8ClampedArray);
```

The pairing applies after aliases and optional parameters are
flattened, and to a generic member such as `Uint8Array<ArrayBuffer>`.
A trailing `undefined` is omission and `null` is an arm of its own.
Where `undefined` cannot be omitted the view keeps it (`Option<&[u8]>`).
`Clamped` has no `Option` form, so such a `Uint8ClampedArray` view keeps
no empty and `undefined` goes through the handle
(`Option<&Uint8ClampedArray>`).
It needs exactly one typed array whose element matches the array's
(`bigint` for the 64-bit arrays, `number` for the rest), except that
`Uint8Array | Uint8ClampedArray | number[]` pairs as the `Uint8Array`
slice, since both hold `u8`. `Float16Array` has no Rust slice and
doesn't pair. Modes are per-argument, so a `string[]` or `number[]`
parameter in the same overload doesn't affect the typed slice.

A bare typed array stays the `&Uint8Array` handle: only the union
says the callee also accepts a copy. A bare `bigint[]` stays
`&[BigInt]` / `Vec<BigInt>`, like a scalar `bigint`. A `Vec<i64>`
return would silently wrap values beyond 64 bits, and the argument
matches it so a getter's value can be passed back to the setter.

### Typed-array buffer arguments

Since TS 5.7 the typed arrays, `DataView` and `ArrayBufferView` are
generic over their buffer (`Uint8Array<ArrayBuffer>`,
`Float64Array<ArrayBufferLike>`). The js-sys types aren't, so the
argument is dropped in every position without a warning, and
`Uint8Array<ArrayBuffer>` lowers exactly like `Uint8Array`.

## Function types

A function type lowers to wasm-bindgen's typed `Function<fn(P…) -> R>`,
with parameters and return in their inside-generic forms (`Number`,
`JsString`, `Boolean`, `Undefined` for `void`), borrowed in argument
position:

```ts
onTick(callback: (tick: number, label: string) => boolean): void;
```

```rust
pub fn on_tick(this: &Timer, callback: &Function<fn(Number, JsString) -> Boolean>);
```

Two shapes lower to the untyped `Function` instead, which any
`js_sys::Function` (a `Promise` executor's `resolve`, a `Closure`'s
`as_ref()`) satisfies without a cast:

* A signature whose parameters and return are all `any` / `unknown`
  carries no type information.
* A signature with a rest parameter (`(...args: any[]) => void`,
  `(first: string, ...rest: number[]) => void`): a `fn` pointer type
  has fixed arity, so there is no typed spelling of it.

Parameters beyond the eighth are dropped from the typed form.

## Outer-type ABI fan-out

Some Rust-idiomatic outer-type lowerings imply a copy across the
Wasm boundary for large data:

* `&str` arguments incur a UTF-8 conversion vs the JS-side
  `String`/`JsString` representation.
* `&[String]` / `&[f64]` arguments are copied into a JS `Array`
  (`&[f64]` via `slice = "array"`).

Rather than emit dual variants for every callable, ts-gen
**defers to the TS author** to pick the form. Each ABI choice
is exposed as a distinct TS spelling:

| TS arg form        | Rust arg-pos       | Rust return-pos | Notes                                  |
| ------------------ | ------------------ | --------------- | -------------------------------------- |
| `string`           | `&str`             | `String`        | Idiomatic Rust; UTF-8 copy on boundary |
| `String`           | `&JsString`        | `JsString`      | Zero-copy JS handle                    |
| `T[]` / `Array<T>` / `ReadonlyArray<T>` | `&[T]` | `Vec<T>` | Idiomatic Rust slice / owned vec |
| `ArrayLike<T>`     | `&Array<U>`        | `Array<U>`      | Zero-copy JS array handle              |

Where `U` is the element's inner-position lowering (`JsString`
for `string`, `Number` for `number`, `Foo` for a named ref).

Writing `String` (the JS wrapper class) gets `&JsString`; writing
`string` (the primitive) gets `&str`. Likewise `ArrayLike<T>` (a
genuinely wider type) gets the JS-array reference form, while the
array types themselves get the Rust slice form — `Array<T>` and
`T[]` are one TS type, so their spelling carries no intent. The TS
spelling controls the choice 1:1 — no fan-out, no duplicate
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
  doc comment. The same name under different `module = "..."` specifiers
  or `js_namespace`s (`Ns.x` and `ns.x`, which share `mod ns`) is a
  distinct binding.
* Distinct JS names that collide after conversion and sanitisation are
  numbered. The name goes to the JS name spelled the same in Rust, then
  to one that only changes case, then to one that loses or spells out
  characters, and within each of those to the first declared. So `ajax`
  keeps `ajax` and `$ajax` becomes `ajax_2` in either order, and
  reordering a `.d.ts` only renames bindings within one of those groups.
  A declared name also outranks a derived one: a method `try_count()`
  keeps its name, and `count()`'s fallible variant becomes `try_count_2`.
  * Free functions and statics share their module's value namespace: a
    function `foo_bar` and statics `fooBar` and `FOO_BAR` become
    `foo_bar`, `foo_bar_2` and `foo_bar_3`.
  * A type's properties and methods are named together. The getter,
    `set_` setter, and a dictionary's factory parameter and builder method
    all follow the property's name (`data` and `$data` give `data` /
    `set_data` and `data_2` / `set_data_2`). The setter's name is derived,
    so it yields to a declared one and is numbered alone: a method
    `setTimeout()` keeps `set_timeout` and a property `timeout` beside it
    gets `timeout` / `set_timeout_2`. A read-only property has no setter to
    number.
  * A property named like one of the type's generated names, a
    constructor's `new` or a dictionary's `new`, `builder` and `build`,
    can only clash with that one name, so it takes a trailing underscore
    instead: `new_` / `set_new`, with `new_` as its factory parameter and
    builder method. A method's derived names (`try_new`, `new_with_x`)
    can clash more than once, so a method `new()` is numbered `new_2`.
  * Instance and static members share one extern block, and an instance
    member keeps its name whatever the declaration order: a static named
    like one takes `_static`, MDN's spelling for the same clash.
    `Response.json(data)` → `json_static(data)` /
    `json_static_with_init(data, init)` beside `response.json()` →
    `json(this)`, and a `static readonly ok` beside an instance `ok` →
    `ok_static()`.
  * A parameter can't shadow a static (E0530), so one named like a static
    in scope, including one a namespace's module imports from its parent,
    takes a trailing underscore (`url_`). So does a dictionary helper's
    parameter named like its `inner` local. wasm-bindgen's expansion and
    the dictionary helpers bind some names themselves (`val`, `inner`,
    `value`, …), so a static with one of those takes the underscore instead
    (`val_`, with `js_name = "val"`).
  * A parameter list numbers without the underscore (`a` and `$a` give
    `a` and `a2`), since wasm-bindgen's generated code names the ABI
    parts of a parameter `a` `a_1` to `a_4`. A method's parameters skip
    its receiver's `this` (`method(_this)` gives `this2`).
* Reserved Rust keywords in fn, method, and accessor names get a trailing
  underscore (`type()` → `type_`). Elsewhere (parameters, variables) they
  are emitted as raw identifiers (`r#type`, `r#loop`), except `self`,
  `Self`, `super`, and `crate`, which cannot be raw and become `self_` etc.
* Names that are not valid Rust identifiers are sanitised (`0` → `_0`,
  `$public` → `public`, `@cf/openai/whisper` → `cfopenaiwhisper`).
  Names with no alphanumeric characters would sanitise to nothing, so
  their symbols are spelled out instead: `$` → `dollar`, `$$` →
  `dollar_dollar`, `_` → `underscore`, and `DollarUnderscore` for a type
  or enum variant named `$_`.
* `js_name = "..."` is emitted whenever the final Rust ident (after all of
  the above) no longer spells the JS name, so `wasm-bindgen` binds to the
  correct runtime name. This applies to types (other than
  [shapes](#shapes-have-no-runtime-class), which name no JS class),
  methods, free functions, variables, getters, and setters (whose JS name `wasm-bindgen` infers by
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

The bindings also spell the Rust prelude bare (`Result<T, JsValue>` on
every fallible binding, `String`, `Option<T>`, `From`/`Default`/`Clone`
in impls), so a declaration named like anything the 2021 prelude exports
(`Result`, `Option`, `Box`, `Default`, `From`, …; the full list is
`RUST_PRELUDE_RESERVED` in `codegen/typemap.rs`) is suffixed the same
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

Every constructor overload (`new`, `new_with_name`, …) binds the same
attributes. wasm-bindgen calls `new <class>(…)` for a constructor, the
class taken from `js_class` or the type it returns, never from the
function's own `js_name`, so none is emitted:

```rust
#[wasm_bindgen(constructor, js_namespace = "Temporal", catch)]
pub fn new(iso_year: f64, iso_month: f64, iso_day: f64) -> Result<PlainDate, JsValue>;
#[wasm_bindgen(constructor, js_namespace = "Temporal", catch)]
pub fn new_with_calendar(/* … */) -> Result<PlainDate, JsValue>;
```

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
their members union, their `extends` lists merge. An interface also merges
into a same-named class; two classes, or an interface and a type alias or
enum, don't (see [Duplicate identifiers](#duplicate-identifiers)). As in
TypeScript, a method's overloads accumulate across the merged declarations
(see [signature flattening](#signature-flattening)), while a property,
getter or setter declared again stays one binding.

### Shapes have no runtime class

A class (including an interface merged with its `declare var`) is a JS
constructor, and `dyn_into` / `is_instance_of` test `instanceof` it. An
interface, dictionary or [discriminated union](#discriminated-unions)
only describes a shape: nothing named `Temporal.RoundingOptions` exists at
runtime, so an `instanceof` check would fail for every value, and in a
module the check would import a class the module doesn't export. Any
object can carry the shape, so the type checks for one instead, and drops
the `js_name` and `js_namespace` that would only have named the missing
class (its members are structural, so they never read them):

```ts
declare namespace Temporal {
  interface RoundingOptions { smallestUnit?: TimeUnit; roundingIncrement?: number; }
}
```

```rust
#[wasm_bindgen(extends = Object, is_type_of = JsValue::is_object)]
pub type RoundingOptions;
```

`JsValue::from(Object::new()).dyn_into::<RoundingOptions>()` succeeds;
a primitive, `null`, `undefined` or a function does not.

`is_type_of` drives `dyn_into`, `dyn_ref` and `has_type`, but not
`JsCast::is_instance_of`, which wasm-bindgen always lowers to a literal
`instanceof` of the type's Rust name, and no attribute overrides it.
For a global or namespaced shape that check is `false` for every value,
and for a shape exported from a module it imports a class the module
doesn't export, which fails the module's load. Test shapes with
`has_type` / `dyn_ref`, never `is_instance_of`.

In a [dynamic-union enum](#erased-return-position-unions-become-dynamic-union-enums)
a shape arm would take every object listed after it, so shape arms
dispatch last; see [Shape arms go last](#shape-arms-go-last).

### Inherited members

Every `extends` parent becomes a wasm-bindgen `extends =`, but
wasm-bindgen derefs to the *first* one only; the others are reachable
through `AsRef` alone. So the members of every parent beyond the first
(and of their ancestors) are declared on the child itself, with the
parent's type parameters substituted by the arguments the `extends`
clause passes. A member the child redeclares wins; between siblings
that declare the same member, the earliest parent in `extends` order
provides it.

```ts
interface Base { a: string; }
interface Geo { country?: string; }
interface Child extends Base, Geo { c: number; }
interface Named extends Tagged, Box<string> {}
```

```rust
#[wasm_bindgen(extends = Base, extends = Geo, extends = Object, is_type_of = JsValue::is_object)]
pub type Child;
pub fn country(this: &Child) -> Option<String>;   // from Geo
pub fn c(this: &Child) -> f64;
pub fn value(this: &Named) -> String;             // Box<T>.value with T = string
```

`Child.a()` is reached through the deref to `Base`. A dictionary's
factory sets the fields of all its parents, first chain included:
`Child::new(a, c)`, `Child::builder(a, c).country(..)`. Classification
also looks through the parents: an interface with no members of its own
that extends dictionaries is a dictionary, one that extends a class-like
parent is class-like.

As in TypeScript, `extends` inherits instance members only: properties,
methods and index signatures. A class parent's constructor and statics
stay on its own JS global, so `interface Handle extends Base, Resource {}`
(or `type Handle = Base & Resource`) declares `Resource`'s `id` and
`close` on `Handle`, but not `Handle::new` or `Handle::open`, which would
call a `Handle` global that doesn't exist.

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
let init = ResponseInit::default();  // the same
```

`impl Default` calls `new()`, so such a dictionary works with
`..Default::default()`-style helpers and `#[derive(Default)]` containers.

If every property is required (no optionals), the builder would carry
only `build()` and is suppressed — only `new(reqs)` is emitted, with
construction inlined. The exception is a dictionary with [literal
factories](#literal-type-discriminator-collapse): each `new_<literal>`
keeps its `builder_<literal>`, whose builder sets the fields the literals
come from, so a value chosen at runtime can replace the inlined one.

```rust
impl TransitionOptions {
    /// Construct with `direction: "next"`.
    pub fn new_next() -> TransitionOptions { Self::builder_next().build() }
    /// Start a builder with `direction: "next"`.
    pub fn builder_next() -> TransitionOptionsBuilder { /* … */ }
    // `new_previous` / `builder_previous` likewise
}
impl TransitionOptionsBuilder {
    pub fn direction(self, val: DirectionKind) -> Self { /* … */ }
    pub fn build(self) -> TransitionOptions { /* … */ }
}
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
SendEmailBuilder::new_with_slice(from: &str, to: &[String], subject: &str)
SendEmailBuilder::new_with_email_address(from: &EmailAddress, to: &str, subject: &str)
SendEmailBuilder::new_with_email_address_and_slice(from: &EmailAddress, to: &[String], subject: &str)
// matching builder*, builder_with_*, etc.
```

### Generated doc comments

Every `new*` and `builder*` variant ships with a doc block listing
exactly what it does. Headings use `##` (h2) and bullets use ` - ` to
separate the field name from its description, matching the format
used elsewhere when JSDoc is rendered to Rust:

* A summary sentence naming each literal baked into the function name
  (these don't appear as parameters): ``Construct with `direction:
  "next"`.``, and ``Start a builder with …`` on a `builder*` variant.
* `## Inlined fields` — bullets `` `field_name: literal_value` `` with
  the field's JSDoc, when one of the baked-in literals' fields has any.
* The caller-supplied fields with JSDoc, as bullets `` `field_name` ``
  in signature order, under `## Arguments` when a section precedes them.

For example:

```rust
/// Construct with `disposition: "inline"`.
///
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

A required field whose type is a *single* literal (`certPresented:
"0"`) offers no choice: the literal is baked into every factory, but
the field does not contribute to the name — only fields with several
options do. A dictionary whose required fields are all single literals
therefore gets a plain `new()`. The branches of a discriminated union
each pin one literal, but across branches the field is the choice, so
it still names each branch's factory (`new_inline` / `new_attachment`
above). The choice is the field's declared option count: when a
dictionary's factory cartesian goes over 64 combinations and each
field is cut to its first option, a literal-union field still names
the one remaining factory (`new_inline`, never a bare `new` that
silently picks `"inline"`).

A number literal is baked in as a float literal matching the `f64`
setter, and names its factory with `.` and a leading `-` spelled out
(`scale: -1 | 0.5` → `new_neg_1()` setting `-1.0`, `new_0_5()` setting
`0.5`).

A literal that the field's [setter takes through its
enum](#literal-union-setters) is passed as that enum's variant instead
(`mode: true | "x" | "y"` → `new_x()` calling
`set_mode(ModeKind::X)`).

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

The merged `set_content_id` takes `&str` (a setter expresses
`undefined` by not being called), so `new_inline` calls it directly.

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
for a union of literals, so its unsuffixed setter and builder method take
that same enum, and the getter's value can be passed straight back. The
enum stands for the literal members it keeps as their own variants:
string literals, and every member of a C-style integer enum. The other
members keep their `_with_<type>` overloads after it.

```ts
interface Options {
  mode: true | "x" | "y";
  inline: "x" | "y";
  level: 0 | 1 | 2;
}
```

```rust
pub fn mode(this: &Options) -> ModeKind;
pub fn set_mode(this: &Options, val: ModeKind);
pub fn set_mode_with_bool(this: &Options, val: bool);
pub fn set_inline(this: &Options, val: InlineKind);
pub fn set_level(this: &Options, val: LevelKind);
```

* A widened `&str` or `f64` setter would accept values outside the union,
  the enum does not.
* String literals covered by a `string` member stay on the `&str` setter,
  which is already exact: `mode: "x" | string` keeps `set_mode(&str)`.
* Number and boolean literals in a mixed union widen inside the enum too
  (`F64(f64)`, `Bool(bool)`). However many there are, they share one
  `f64` / `bool` overload: `digits?: "auto" | 0 | 1 | … | 9` gives
  `set_digits(DigitsKind)` and `set_digits_with_f64(f64)`, where the
  ten literals as separate alternatives would overflow the fan-out limit
  and leave only a `&JsValue` setter.
* Unions without literal members keep their per-type overloads.
* Builder methods for optional fields follow their setter.

## Anonymous interface synthesis

An inline `{ … }` type — or a union or intersection built from `{ … }`
types and named references — that appears in a position where a named
interface would do is promoted to a real `InterfaceDecl` so consumers
get a typed builder rather than an opaque `Object`. Three positions
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

### Method return position

```ts
declare class Cursor<T> {
  next(): { done?: false; value: T } | { done: true; value?: never };
}
```

is treated as if the user had written `interface CursorNextReturn<T>
{ done?: boolean; value?: T }` and `next(): CursorNextReturn<T>`. The
name takes the `Return` segment used by synthesized return enums.
Enclosing type parameters that the literal mentions become the hoisted
interface's own parameters, instantiated at the reference, so its
getters and builder bind them. Free functions have no parent name and
keep the `Object` erasure, as their parameters do.

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
`Record<…>`, etc.) keep their existing alias semantics. The promoted
interface is still an alias to TypeScript, so a same-named `interface`
doesn't merge into it (see [Duplicate identifiers](#duplicate-identifiers)).

### Union of inline literals

When every branch of a union is itself an inline literal, the branches
are **structurally merged** into a single member set. The merge
covers all three positions above:

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
members. Through [inherited members](#inherited-members) every part's
members are reachable on the result: `cf.country()` resolves on
`IncomingRequestCfProperties`, and `EmailMessageBuilder =
EmailReplyMessageBuilder & EmailDestinations` gets a factory over both
parts' fields. This is what makes the common required-field refinement
produce a usable type:

```ts
interface R2Bucket {
  get(key: string, options: R2GetOptions & { onlyIf: R2Conditional | Headers }): …;
  get(key: string, options?: R2GetOptions): …;
}
type D1Result<T = unknown> = D1Response & { results: T[] };
```

```rust
#[wasm_bindgen(extends = R2GetOptions, extends = Object, is_type_of = JsValue::is_object)]
pub type R2BucketOptions;
pub fn new(only_if: &R2Conditional) -> R2BucketOptions;
pub fn new_with_headers(only_if: &Headers) -> R2BucketOptions;

#[wasm_bindgen(extends = D1Response, extends = Object, is_type_of = JsValue::is_object)]
pub type D1Result<T>;
pub fn results<T>(this: &D1Result<T>) -> Vec<T>;
```

Several literals in one intersection are merged with the **meet** of
the union rules above (a value is every branch at once): a property is
required if any branch requires it, writable if any branch allows
writing, and a property typed differently across branches carries the
intersection of those types, which is bounded at codegen (see
[Intersections take their most specific member](#intersections-take-their-most-specific-member)).

A parenthesised union of literals as a part — `R2Objects`' `& ({
truncated: true; cursor: string } | { truncated: false })` — merges
like a [union of inline literals](#union-of-inline-literals): `cursor`
becomes optional. Members declared alongside win over the union's
narrowing, so `truncated` keeps the explicit `boolean`.

Every parent must be something an interface could `extends`. A type
parameter (`T & { id: string }`) or an alias to a non-object type
(`type Id = string`) leaves the intersection unhoisted, to be bounded
at codegen. Aliases to other names are followed (`type Id2 = Id`), and
so are the parents of an intersection alias, so `Id & { … }` as a parent
refuses too. A generic alias whose target mentions its own type
parameters (`type Branded<T> = T & { … }`, `type Same<T> = T`,
`type Of<T> = Provider<T>`) is refused, since its arguments are not
substituted into the target. Imported and `--external` names have no local binding and
are allowed through. A part that only narrows in TS (a conditional
or indexed-access type, or a union other than of literals, see
below; parenthesised or not) has no structural reading and is dropped
with a warning, and `object` / `unknown` add
nothing, so `(T extends X ? … : unknown) & { fetch(): … }` is still an
interface with `fetch`. A parent that erases at codegen (a name that
never resolves and is reported) is dropped from `extends` rather than
replaced, so the first real parent stays the `Deref` target; the
literal's own members survive either way. A generic first parent is
spelled bare in `extends`, which wasm-bindgen reads as
`Parent<JsValue>`; the type still derives `Eq` (see
[Type-level generics](#type-level-generics-propagate-to-every-binding-site)).

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
Union members that erase to the same type are one member, so a
property typed `{ … } | { … }` is `Option<Object>`, and `{ … } | { … }
| string` is a two-variant `Object` / `String` dynamic union rather
than carrying an unreachable duplicate variant.
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

The setter each factory calls is the merged shape's, so a discriminant
the merged setter takes as an enum is passed as that enum's variant,
though each branch narrows it to one literal:

```rust
pub fn set_disposition(this: &EmailAttachment, val: DispositionKind);
// in new_inline:
inner.set_disposition(DispositionKind::Inline);
// in Status::new_200:
inner.set_code(CodeKind::N200);
```

Likewise a field a branch narrows to one member of the merged union
(`data: ArrayBuffer` of `string | ArrayBuffer`) calls that member's
overload, `inner.set_data_with_array_buffer(data)`, not the first one.

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

is bound as TypeScript binds it: the interface and the variable are two
declarations of one symbol in their container, which may be a file's
top level, a namespace, a `declare module` body or the global scope
across files. The interface contributes the instance members and the
variable's type the constructor and statics, and the symbol emits as a
single class in that container, whichever is declared first.

```ts
declare namespace R {
  interface Q { q(): void; }
  var Q: { prototype: Q; new (): Q };
}
```

```rust
pub mod r {
    #[wasm_bindgen]
    extern "C" {
        #[wasm_bindgen(extends = Object, js_namespace = "R")]
        pub type Q;
        #[wasm_bindgen(constructor, js_namespace = "R", catch)]
        pub fn new() -> Result<Q, JsValue>;
        #[wasm_bindgen(method)]
        pub fn q(this: &Q);
    }
}
```

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

As TypeScript's checker types a declaration, the variable's type is
resolved once every file is bound, from the variable's own location:
`URIErrorConstructor`, or a qualified `N.URIErrorConstructor`, resolves
from the variable's scope with every file's declarations of it merged,
wherever it is declared. Its constructors and statics resolve from
the constructor interface, so a `Loc` in `FooCtor` below is the global
one even if `N` declares a `Loc` of its own. Only
declarations in the same container merge into the class, so an
`interface URIError` inside a `declare module` is a different
declaration and leaves the global class alone, and a constructor
interface declared outside a namespace still gives the namespace's
class its constructor and statics:

```ts
interface FooCtor { new (): N.Foo; readonly prototype: N.Foo; create(): N.Foo; }
declare namespace N {
  interface Foo { go(): void; }
  var Foo: FooCtor;
}
```

emits `n::Foo` with `new`, `create` and `go`, binding through
`js_namespace = "N"`. A module's constructor variable with no interface
of its own is a class of its own (see below), taking its constructor
and statics the same way. A global or namespace variable with no
interface of its name is a value of the constructor interface's type,
which stays an interface.

A variable typed by an inline object literal is a class even without a
same-named interface when the literal has constructor shape (a `new`
signature or a `prototype` member). The construct signature's return
type then describes the instance: an inline literal supplies the
instance members, a named type becomes the parent:

```ts
declare const WebSocketPair: { new (): { 0: WebSocket; 1: WebSocket } };
declare const TcpSocket: { new (address: string): Socket };
```

```rust
pub type WebSocketPair;
pub fn new() -> Result<WebSocketPair, JsValue>;
#[wasm_bindgen(method, getter, js_name = "0")]
pub fn _0(this: &WebSocketPair) -> WebSocket;

#[wasm_bindgen(extends = Socket, extends = Object)]
pub type TcpSocket;
```

A same-named interface in another file replaces that instance, as it
does within one file.

A construct signature on any other object type (`interface Factory {
new (): Widget }`, or an inline `{ new (): T }` parameter type) is the
type of a constructor *value*, not a constructor of that type. It is
dropped with a warning rather than emitted as
`#[wasm_bindgen(constructor)]` on the wrong class.

A construct signature may declare its own parameters —
`new <T>(initial: T): MyClass<T>` — which bind the class parameter of
the same name, so the constructor lowers as
`new<T: JsGeneric>(initial: &T) -> Result<MyClass<T>, JsValue>` (see
[Type-level generics](#type-level-generics-propagate-to-every-binding-site)
for the mentioned-only rule). The interface's type parameters and body
scope carry over to the merged class.

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

Upstream pairs this with a **global** `interface EmailMessage { … }`
holding the instance shape (the constructor's return type names it).
The two collapse into the one module-scoped class: the interface's
members (and `extends`) move onto the class, the interface is dropped,
and every reference to `EmailMessage` — `ForwardableEmailMessage
extends EmailMessage`, `reply(message: EmailMessage)` — resolves to the
class. A global reference is therefore a reference into
`cloudflare:email` (`cloudflare::email::EmailMessage`), like any other
[module-context declaration](#module-declarations-and-namespace-nesting).
The interface is the one the construct signature's return type resolves
to from the module, so another module's own `EmailMessage` is untouched.
The collapse runs once every file's declarations have merged, so neither
file nor declaration order matters. The interface of a class exported
under its own name (`export let Email: { new (): Email }`) is not
collapsed.

### Constructor variables named by a type alias

```ts
declare module "cloudflare:workers" {
  export type RpcStub<T extends Rpc.Stubable> = Rpc.Stub<T>;
  export const RpcStub: {
    new <T extends Rpc.Stubable>(value: T): Rpc.Stub<T>;
  };
}
```

When a type alias in the same scope shares the variable's name, the alias
stays the type, so `RpcStub<T>` remains `Rpc.Stub<T>` everywhere. The
construct signatures become constructors of the types they return, with
`js_class` naming the variable, and keep their own type parameters:

```rust
pub type RpcStub<T> = super::super::rpc::Stub<T>;
#[wasm_bindgen(module = "cloudflare:workers")]
extern "C" {
    #[wasm_bindgen(constructor, js_class = "RpcStub", catch)]
    pub fn new<T: JsGeneric>(value: &T) -> Result<super::super::rpc::Stub<T>, JsValue>;
}
```

`RpcStub::new(&value)?` runs `new RpcStub(value)` and returns a stub that
any `Rpc.Stub<T>` parameter accepts. Overloads constructing the same type
are named like a class's (`new`, `new_with_*`), and each type takes its
own names. Each returned type must be an emitted extern type without a
`new` of its own. A class constructor, a dictionary's builder, a member
named `new`, or another variable already constructing that type would
collide. A literal with members besides its construct signatures and
`prototype` would leave them unreachable. In those cases the variable
binds as a value instead (`pub static rpc_stub: Object`). Declaration
order does not matter.

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

A function's declarations are its overloads wherever they merge:
within a file, within a namespace, and across input files. Each
overload of a function or method is a separate declaration, as in
TypeScript, so each expansion keeps its own overload's return type,
type parameters, JSDoc and `@throws`.

```ts
// a.d.ts
declare function parse(input: string): object;
declare function parse(input: ArrayBuffer): object;
// b.d.ts
declare function parse(input: string, reviver: Function): object;
```

```rust
pub fn parse(input: &str) -> Object;
#[wasm_bindgen(js_name = "parse")]
pub fn parse_with_array_buffer(input: &ArrayBuffer) -> Object;
#[wasm_bindgen(js_name = "parse")]
pub fn parse_with_reviver(input: &str, reviver: &Function) -> Object;
```

Methods, static methods and constructors are the same: every merged
declaration of a class or interface contributes its overloads, whether
the declarations are interfaces, an interface and a class, or
constructor interfaces supplying a class's static side. As in
TypeScript's overload resolution, the later declaration's overloads come
first, each declaration's in its own order, so the later one gets the
bare name. An overload declared identically twice is one binding, and
when two declare the same parameters, the later one's return type wins.

```ts
interface Stream { write(data: string): void; now(): number; }
interface Stream { write(data: number): void; write(data: string): void; now(): Date; }
```

```rust
#[wasm_bindgen(method)]
pub fn write(this: &Stream, data: f64);
#[wasm_bindgen(method, js_name = "write")]
pub fn write_with_str(this: &Stream, data: &str);
#[wasm_bindgen(method)]
pub fn now(this: &Stream) -> Date;
```

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
   A trailing parameter admitting `undefined` truncates like an
   optional one. A parameter typed by a named union alias does not
   expand: it takes the alias's enum, or its trait under
   `--experimental-generic-mono` (see
   [named union aliases](#named-union-aliases)). The
   expansion is the same under
   `--experimental-generic-mono` (see
   [primitive union arguments](#primitive-union-arguments)).
2. **Cross-overload dedup**: When multiple overloads expand to the same
   concrete parameter list, drop the duplicates. Two overloads that
   both truncate to `(callback)` produce only one binding. Duplicates
   that differ only in the value of string-literal parameters are kept
   as arms of one binding instead (see
   [literal-keyed overloads](#inline-wrappers)).
3. **Suffix assignment**: Across all surviving expansions, compute
   `_with_X` / `_with_X_and_Y` suffixes that disambiguate them. The
   primary variant gets `""`; the others are named after the
   parameters that distinguish them (see
   [variant names](#variant-names)).

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
pub fn show_with_str(value: &str);
pub fn show_with_f64(value: f64);
pub fn show_with_str_and_opts(value: &str, opts: &ShowOpts);
pub fn show_with_f64_and_opts(value: f64, opts: &ShowOpts);
```

`compute_rust_names` in `codegen::signatures` handles the suffix
disambiguation, including readability adjustments when the same
parameter name appears in multiple alternatives.

### Variant names

A variant's suffix names what it adds beyond the bare form: the
parameters where it departs from the primary, by name where the primary
lacks the parameter, by type where a sibling has the same parameter with
another type.

```ts
until(other: PlainDate | string, options?: DifferenceOptions): Duration;
```

```rust
pub fn until(this: &PlainDate, other: &PlainDate) -> Duration;
pub fn until_with_str(this: &PlainDate, other: &str) -> Duration;
pub fn until_with_options(this: &PlainDate, other: &PlainDate, options: &DifferenceOptions) -> Duration;
pub fn until_with_str_and_options(this: &PlainDate, other: &str, options: &DifferenceOptions) -> Duration;
```

The `other` of `until_with_options` is the primary's own, so it goes
unnamed; `until_with_str_and_options` departs in both. Where two
variants would add the same thing at different positions, each is named
in full instead: `compare(one: A | B, two: A | B)` gives
`compare_with_b_and_a` and `compare_with_a_and_b`, never a lone
`compare_with_b` for one of them.

The same holds across overloads: `put(key, value, options?)` beside
`put(entries, options?)` gives `put_with_options` for the first with
options, and `put_with_entries` and `put_with_entries_and_options` for
the second, which departs from the primary in its parameter names.

A base name that already reads `with` (`with`, `set_with`) continues the
suffix with `_and_`: `with(fields, options?)` gives `with` and
`with_and_options`, not `with_with_options`.

### Named union aliases

A named alias of a union is one Rust type, not one binding per arm. The
alias becomes a wasm-bindgen enum with a tuple variant per arm, which
crosses the ABI as the arm's own value, with `From` conversions from
each arm's natural Rust argument, and a parameter of the alias's type
takes `impl Into<Alias>`. Where a read can tell the arms apart, the same
enum is what a read of the alias returns, so a value read can be passed
straight back:

```ts
declare namespace Temporal {
  type PlainDateLike = PlainDate | ZonedDateTime | PlainDateTime | DateLikeObject | string;
  type TimeZoneLike = ZonedDateTime | string;
  interface PlainDateConstructor {
    from(item: PlainDateLike, options?: OverflowOptions): PlainDate;
    compare(one: PlainDateLike, two: PlainDateLike): number;
  }
  interface InstantToStringOptions {
    timeZone?: TimeZoneLike | undefined;
  }
}
```

```rust
#[wasm_bindgen(private, fallback)]
#[derive(Clone, Debug)]
pub enum PlainDateLike {
    PlainDate(PlainDate),
    ZonedDateTime(ZonedDateTime),
    PlainDateTime(PlainDateTime),
    String(String),
    DateLikeObject(DateLikeObject),
}
impl From<PlainDate> for PlainDateLike { /* … */ }
impl From<&PlainDate> for PlainDateLike { /* a clone of the handle */ }
// … each class and dictionary arm likewise
impl From<&str> for PlainDateLike { /* … */ }
impl From<String> for PlainDateLike { /* … */ }
impl From<&String> for PlainDateLike { /* … */ }
impl From<JsString> for PlainDateLike { /* … */ }
impl From<&JsString> for PlainDateLike { /* … */ }

extern "C" {
    #[wasm_bindgen(static_method_of = PlainDate, js_name = "compare")]
    fn __compare(one: PlainDateLike, two: PlainDateLike) -> f64;
    // …
    #[wasm_bindgen(method, getter, js_name = "timeZone")]
    pub fn time_zone(this: &InstantToStringOptions) -> Option<TimeZoneLike>;
    #[wasm_bindgen(method, setter, js_name = "timeZone")]
    fn __set_time_zone(this: &InstantToStringOptions, val: TimeZoneLike);
}
impl PlainDate {
    pub fn from(item: impl Into<PlainDateLike>) -> PlainDate { Self::__from(item.into()) }
    pub fn from_with_options(item: impl Into<PlainDateLike>, options: &OverflowOptions) -> PlainDate;
    pub fn compare(one: impl Into<PlainDateLike>, two: impl Into<PlainDateLike>) -> f64 {
        Self::__compare(one.into(), two.into())
    }
}
impl InstantToStringOptions {
    pub fn set_time_zone(&self, val: impl Into<TimeZoneLike>) {
        self.__set_time_zone(val.into())
    }
}
```

`PlainDate::compare(&date, "2024-01-01")` and
`PlainDate::compare(DateLikeObject::new(1.0), &zoned)` both call the one
binding, where the expansion used to emit 25 (and as many `try_`
companions), each named for its two arm types, and
`options.set_time_zone(other.time_zone().unwrap())` passes a read back.

* **Which aliases.** A local, non-generic alias whose target, through
  nested aliases, is a union of two or more value arms once `null` and
  `undefined` are set aside, each arm a string, number, boolean,
  `bigint`, `symbol`, `object`, or a class or interface without type
  parameters that the output binds. Anything else lowers as before: an
  anonymous union (`f(x: Foo | string)`), a single-arm alias, a generic
  alias (`type Info<M> = Request<M> | string`, see
  [type aliases](#type-aliases-and-export--x-as-y-)), a union of string
  literals (an enum), or an alias with a literal, array or function arm.
* **Shared or argument-only.** The enum is the alias's read type too
  when a read would [tell its arms apart](#arms-a-read-can-tell-apart)
  as it would the union written out: classes by `instanceof`, primitives
  by `typeof`, and at most one shape arm (a dictionary, `object`), which
  goes last and makes the enum `#[wasm_bindgen(fallback)]`. An alias
  with several shape arms (`type Fields = ZonedFields | DayFields`), or
  whose arms share a named upper bound a read keeps instead
  (`TypeError | RangeError` reads as `Error`), is an argument enum only,
  and its reads resolve it to its members as before. An alias that is
  only ever read still uses its enum; there is no separate read enum.
* **Variants and conversions.** Variants and payloads are a
  [dynamic union](#variant-kinds-and-naming)'s: a string arm is
  `String(String)`, `number` and `boolean` are `F64(f64)` and
  `Bool(bool)`, and two arms with the same payload are one variant. A
  class, interface, `bigint`, `symbol` or `object` arm converts from the
  handle and from a reference to it; a string arm from `&str`, `String`,
  `&String`, `JsString` and `&JsString`; `number` and `boolean` from
  `f64` and `bool`. The enum replaces the `pub type Alias = JsValue` the
  alias used to emit.
* **Nullability.** The alias's empties follow the
  [nullability model](#writes): `null` is a `_with_null` arm of its own,
  and `undefined` omits a trailing parameter. A parameter that admits
  `undefined` but cannot be omitted takes `Option<Alias>`, since `None`
  would give an `impl Into` nothing to infer from. A read takes the
  carrier for the empties of the use site and of the alias
  (`Option<TimeZoneLike>`).
* **Positions.** Every write position: function, method, static and
  constructor parameters, setters, and the dictionary factories and
  builders over them (`Span::new(start: impl Into<DayLike>)`). The
  extern binding is private (`__<name>`) and takes the enum by value; a
  [wrapper](#inline-wrappers) of the public name converts. Getters and
  returns of the alias itself read its enum; the alias inside a generic
  container (`Array<PlainDateLike>`) keeps its lowering.
* **Anonymous unions of aliases.** `relativeTo?: ZonedDateTimeLike |
  PlainDateLike` has no name of its own: its read is the
  [flattened](#union-aliases-flatten-into-the-read) getter-named
  `RelativeToKind`, and its setter expands per alias, each alias one
  arm, `set_relative_to(impl Into<ZonedDateTimeLike>)` and
  `set_relative_to_with_plain_date_like(impl Into<PlainDateLike>)`. The
  read enum can't stand in for them: its two dictionaries share one
  untyped `Object` arm, which would take any object.

Under `--experimental-generic-mono` the alias's name is a trait instead,
which each arm's own argument types implement, so an argument takes
`impl Alias` and wasm-bindgen monomorphises each call to the arm's own
ABI, with no conversion and no wrapper. The enum is `<Alias>Kind`,
emitted for every such alias, readable or not, with the same variants
and `From` conversions, and it implements the trait too, so a read, or
an enum built explicitly, is accepted wherever the alias is. rustc lists
only a few implementors in an unsatisfied-bound error, and a string arm
alone has seven, so the trait's `on_unimplemented` names the alias's arms
instead:

```rust
#[diagnostic::on_unimplemented(
    message = "`{Self}` is not a `PlainDateLike`",
    label = "expected PlainDate, ZonedDateTime, PlainDateTime, string or DateLikeObject"
)]
pub trait PlainDateLike: ::wasm_bindgen::convert::IntoWasmAbi {}
impl PlainDateLike for PlainDate {}
impl PlainDateLike for &PlainDate {}
// … each class and dictionary arm likewise
impl PlainDateLike for &str {}
impl PlainDateLike for String {}
impl PlainDateLike for &String {}
impl PlainDateLike for &Box<str> {}
impl PlainDateLike for &Cow<'_, str> {}
impl PlainDateLike for JsString {}
impl PlainDateLike for &JsString {}
impl PlainDateLike for PlainDateLikeKind {}

#[wasm_bindgen(private, fallback)]
#[derive(Clone, Debug)]
pub enum PlainDateLikeKind { /* as above */ }

extern "C" {
    #[wasm_bindgen(static_method_of = PlainDate)]
    pub fn compare(one: impl PlainDateLike, two: impl PlainDateLike) -> f64;
    #[wasm_bindgen(method, getter, js_name = "timeZone")]
    pub fn time_zone(this: &InstantToStringOptions) -> Option<TimeZoneLikeKind>;
    #[wasm_bindgen(method, setter, js_name = "timeZone")]
    pub fn set_time_zone(this: &InstantToStringOptions, val: impl TimeZoneLike);
}
```

* A string arm takes what `JsStringLike` does; `number` and `boolean`
  take `f64` and `bool`; every other arm takes the handle and a
  reference to it. Nested aliases flatten into one implementation per
  arm.
* A parameter that admits `undefined` but cannot be omitted takes
  `Option<AliasKind>`: the trait asks only `IntoWasmAbi` of its
  implementations, since `f64` has no `OptionIntoWasmAbi`.
* `<Alias>Kind` claims its name before any read enum is named, so a
  getter-named enum that would collide is the one prefixed.

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
| `A.X` (qualified) | `&a::X`        | `_with_a_x`     |
| `null`          | (dropped — see [Optional and nullable types](#optional-and-nullable-types)) | – |
| `undefined`     | (dropped, same)  | – |
| `any` / `unknown` | `&JsValue`     | `_with_js_value`|

For named JS types the snake-cased name doubles as the Rust
identifier (`Uint8Array` → `uint8_array`, also `&Uint8Array` in
the rendered type), so there's no separate translation table —
the same convention works in both directions. A qualified name
snake-cases its full path, like its Rust path (`A.X` → `a::X` →
`a_x`), so `A.X` and `B.X` don't collide.

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

### `--fallible` — single `Result` form

TypeScript carries no throw information, so the pair above is emitted for
every sync method and function. A surface that wants the `Result` form
everywhere pays double the API, and the trapping primary is a footgun.
`--fallible` (`GenerateOptions::fallible(true)`) folds the pair into one
binding under the primary name:

```rust
// default
#[wasm_bindgen(method)]
pub fn frobnicate(this: &Foo) -> String;
#[wasm_bindgen(method, catch, js_name = "frobnicate")]
pub fn try_frobnicate(this: &Foo) -> Result<String, JsValue>;

// --fallible
#[wasm_bindgen(method, catch)]
pub fn frobnicate(this: &Foo) -> Result<String, JsValue>;
```

Every expansion that would have had a `try_` companion (flattened
overloads, `_with_` forms, the `_js_string` return variant) becomes a
single catching form; no `try_` name is declared. The error type is
unchanged: `@throws {T}` → `T`, otherwise `JsValue`, or `js_sys::Error`
under `--errors-as-error`, so the two flags compose. `@throws {never}`
keeps a sync binding infallible (no `catch`, plain `T`) and is the only
way to get one under the flag. Async callables, constructors (always
catch), setters (never catch) and property getters are not affected
(a getter annotated `@throws {X}` catches under either policy; see
[`@throws {X}` on getters](#throws-x-on-getters)). An async callable's
sync `<name>_promise` form (below) catches under the flag like any
other sync primary, unless calling it
[never throws synchronously](#builtins-globals-and-async-never-throw-synchronously).
A parameterless `toString()` keeps its `try_to_string` name, since
`to_string` is [`ToString`'s](#tostring-is-display).
Without the flag, output is identical to the default.

### `never` returns keep only the catching form

A callable whose return type is `never` cannot complete normally: it
throws (`valueOf(): never` on the Temporal classes), or never returns at
all (`process.exit(): never`, an assertion helper). A plain binding
could only trap, so a function, method or static method returning
`never`, a variable typed as such a function included, emits only its
catching form, and its success type is `()`. That form takes the bare
name under every policy: `try_` marks the catching twin of a plain
form, and here there is no plain form for it to tell apart.

```ts
declare class PlainDate {
  valueOf(): never;
}
declare function rejectLater(message: string): Promise<never>;
```

```rust
// default and --fallible alike: no plain form, no `try_value_of`
#[wasm_bindgen(method, catch, js_name = "valueOf")]
pub fn value_of(this: &PlainDate) -> Result<(), JsValue>;

// `Promise<never>`: the usual async pair, awaiting `()`
#[wasm_bindgen(catch, js_name = "rejectLater")]
pub async fn reject_later(message: &str) -> Result<(), JsValue>;
#[wasm_bindgen(js_name = "rejectLater")]
pub fn reject_later_promise(message: &str) -> Promise<Undefined>;
```

* The callable is still bound: one that never returns without throwing
  is still worth calling, and `Ok(())` is simply never observed.
* The error type follows the usual rules: `@throws {T}` → `T`,
  otherwise the `--error-type` default.
* `@throws {never}` contradicts a `never` return. The return type wins
  and the annotation is ignored, as it is on a constructor: the binding
  catches with the default error type.
* `Promise<never>` keeps its `async fn` and `_promise` forms (whose
  `catch` follows the [usual policy](#name_promise--the-un-awaited-form)).
  Both are typed as for `Promise<void>`: `Result<(), E>` and
  `Promise<Undefined>`.
* The rule applies per overload and per expansion: with
  `check(value: string): string` and `check(value: number): never`,
  the string form keeps `check` + `try_check`, and the number form is
  a catching `check_with_f64` alone. The arms of a
  [literal-keyed cohort](#inline-wrappers) follow suit: beside
  `read(kind: "text"): string`'s `read_text` + `try_read_text`,
  `read(kind: "fail"): never` gets only a catching `read_fail()` wrapper
  over a catching `__read_fail` binding.
* No `try_` name is reserved for the missing twin.
* Only a literal `never` return counts, not an alias of it. Getters
  and properties typed `never` are unchanged (`JsValue` reads, see
  [Primitive types](#primitive-types)), and so are constructors, which
  always catch.

### `toString()` is `Display`

A parameterless `toString(): string` method is the type's
`core::fmt::Display`. An inherent `to_string(&self) -> String` would
shadow the blanket `ToString` (clippy's `inherent_to_string`), so none
is declared: `format!("{x}")` and `x.to_string()` call JS `toString()`
through `Display`.

```ts
declare class PlainDate {
  toString(options?: PlainDateToStringOptions): string;
}
```

```rust
#[wasm_bindgen(method, catch, js_name = "toString")]
pub fn try_to_string(this: &PlainDate) -> Result<String, JsValue>;
#[wasm_bindgen(method, js_name = "toString")]
pub fn to_string_with_options(this: &PlainDate, options: &PlainDateToStringOptions) -> String;
#[wasm_bindgen(method, catch, js_name = "toString")]
pub fn try_to_string_with_options(/* … */) -> Result<String, JsValue>;

impl core::fmt::Display for PlainDate {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(&self.try_to_string().map_err(|_| core::fmt::Error)?)
    }
}
```

* Only the bare `to_string` binding is replaced. The catching
  `try_to_string` stays, typed by the error model (`@throws`,
  `--error-type`), and is the form that keeps the JS error. Under
  `--fallible` the single catching form is `try_to_string` too, since
  `to_string` belongs to `ToString`. A throwing `toString` makes `fmt`
  return `fmt::Error`, which `format!` and `.to_string()` turn into a
  panic.
* `@throws {never}` leaves no catching form, so `fmt` calls a private
  `__to_string` extern instead.
* The overloads with parameters (`to_string_with_options`) and the
  per-monomorphization `to_string_js_string` variant keep their
  bindings.
* A `toString` that takes a required parameter or doesn't return
  `string` is an ordinary method.
* `Display` is not inherited by a subclass, but `.to_string()` is still
  found on the parent through `Deref`.

## `Promise<T>` returns become `async fn`

```ts
function fetch(url: string): Promise<Response>;
```

emits an async signature with `catch` (beside the sync
[`_promise` form](#name_promise--the-un-awaited-form)):

```rust
#[wasm_bindgen(catch)]
pub async fn fetch(url: &str) -> Result<Response, JsValue>;
```

* The async + catch form is already fallible — no `try_fetch` companion.
* `wasm-bindgen` rewraps the `T` as `Promise<T>` on the JS side.
* Constructors and setters never become async.

### `<name>_promise` — the un-awaited form

An `async fn` import calls into JS only when the returned future is
first polled. Some callers need the JS call to have *started* by the
time the statement ends: a Durable Object constructor gating itself on
`blockConcurrencyWhile`, `ctx.waitUntil(promise)`, or a `Promise.race`
against a signal. Every `Promise`-returning callable therefore also
emits a sync sibling returning the typed promise itself:

```rust
#[wasm_bindgen(catch)]
pub async fn fetch(url: &str) -> Result<Response, JsValue>;
#[wasm_bindgen(js_name = "fetch")]
pub fn fetch_promise(url: &str) -> Promise<Response>;
```

* The suffix is appended after any `_with_X` disambiguator
  (`fetch_with_init_promise`), as `_js_string` is.
* Rejection travels inside the promise (`Promise<T>: IntoFuture<Output
  = Result<T, JsValue>>`), so `catch` on the sync form only covers a
  synchronous throw. Under the default policy the form is plain, like
  every other sync primary; under `--fallible` it catches and returns
  `Result<Promise<T>, E>`, typed by `@throws` like the primary, and
  `@throws {never}` makes it plain again. A builtin, a global or an
  `async`-declared callable keeps it plain under every policy (below).
* A `Promise`-typed *property* is already a sync getter and gets no
  sibling.

#### Builtins, globals and `async` never throw synchronously

An async function of the host reports failure by rejecting the promise
it returns, not by throwing, so calling it never throws synchronously.
Its `_promise` form is a plain `Promise<T>` under `--fallible` too,
whatever its `@throws`: on an async callable `@throws` describes the
rejection, which the `async fn` form keeps carrying. This covers a
callable whose binding is

* **builtin**: it resolves to a module specifier with a `scheme:`
  prefix (`node:fs`, `cloudflare:sockets`, `bun:sqlite`, `workerd:…`),
  or to a bare Node builtin (`fs`, `fs/promises`, `path`, …, Node's
  `require("module").builtinModules`). The scheme is the non-empty text
  before the first `:`. A URL, whose `:` is followed by `//`
  (`https://esm.sh/foo`, `file:///x`), is not a builtin, and neither are
  the schemes `npm:`, `jsr:`, `data:` and `blob:`, which name a package
  or an inline or object URL (`npm:lodash`, `jsr:@std/path`);
* **global**: it has no module, so it lives on the global object: global
  functions, global classes and interfaces with their methods and
  statics, and the functions of global namespaces.

A callable's module is the one its declaration is bound in, the
`module = "…"` its extern block carries: a `declare module "…"` body's,
the `--lib-name` module's for a module file's declarations, and none for
a script file's or a `declare global` block's. Callables of any other
module, such as the `--lib-name` library, a package (`npm:lodash`
included) or a URL, keep the `--fallible` rule.

A function or method declared `async` gets the same: the modifier
promises a rejection rather than a throw. TypeScript rejects `async` in
an ambient context, but ts-gen parses every input, a `.ts` file passed
by name included, as declarations, and the modifier comes through.

```ts
declare global {
  /** @throws {TypeError} on a non-GET request */
  function cachePut(url: string): Promise<void>;
}
declare module "user-lib" {
  export function fetchText(url: string): Promise<string>;
}
```

```rust
// --fallible
#[wasm_bindgen(catch, js_name = "cachePut")]
pub async fn cache_put(url: &str) -> Result<(), TypeError>;
#[wasm_bindgen(js_name = "cachePut")]
pub fn cache_put_promise(url: &str) -> Promise<Undefined>;

// in the `#[wasm_bindgen(module = "user-lib")]` block
#[wasm_bindgen(catch, js_name = "fetchText")]
pub fn fetch_text_promise(url: &str) -> Result<Promise<JsString>, JsValue>;
```

The alternative, making the primary itself return `Promise<T>` and
relying on `IntoFuture`, was rejected: the future's error is always
`JsValue`, discarding `--errors-as-error` and `@throws` on every
async binding, and under `--fallible` a sync throw and a rejection would need
`?.await?`.

### Awaited values lower like sync returns

The resolved value of an `async fn` import does not cross the boundary
as a `Promise<T>` payload: wasm-bindgen awaits the promise through
`JsFuture<T>`, whose resolve closure takes `T` by `FromWasmAbi`, the
same conversion a sync return gets. Async returns therefore use the
top-level return lowering, and only the `_promise` form, which hands
out the JS promise object itself, keeps the inside-generic wrappers:

| TypeScript                      | `async fn`                    | `_promise` form              |
| ------------------------------- | ----------------------------- | ---------------------------- |
| `Promise<boolean>`              | `Result<bool, E>`             | `Promise<Boolean>`           |
| `Promise<number>`               | `Result<f64, E>`              | `Promise<Number>`            |
| `Promise<string>`               | `Result<String, E>`           | `Promise<JsString>`          |
| `Promise<void>`                 | `Result<(), E>`               | `Promise<Undefined>`         |
| `Promise<string \| null>`       | `Result<Option<String>, E>`   | `Promise<JsNullable<JsString>>` |
| `Promise<number \| undefined>`  | `Result<Option<f64>, E>`      | `Promise<JsOption<Number>>`  |
| `Promise<Foo[]>`                | `Result<Vec<Foo>, E>`         | `Promise<Array<Foo>>`        |
| `Promise<A \| B>` (erased)      | `Result<<name>ReturnKind, E>` | `Promise<JsValue>`           |
| `Promise<Foo>` (named JS type)  | `Result<Foo, E>`              | `Promise<Foo>`               |

The awaited value's own empties use the [read carrier](#reads)
(`Option<T>`) around that form; the `_promise` form's are an
[inner position](#inner-positions), which per-monomorphization gives
the read carrier too (`Promise<Option<JsString>>`, see
[the table](#per-monomorphization)). The
exception is a list over an in-scope type parameter: `get<T>():
Promise<T[]>` stays `Result<Array<T>, E>`, because an `async` import
converts the awaited value through `<Vec<T> as FromWasmAbi>`, which
`T: JsGeneric` does not provide, whereas a sync import erases `T` to
its concrete representation first. A JS value that contradicts the
declaration (a number where `Promise<string>` was declared) aborts at
the conversion, exactly as a sync `-> String` import would.

The `_promise` form keeps JS-typed parameters because the promise is a
JS object that flows into JS APIs (`waitUntil`, `Promise.race`), which
need `T: JsGeneric` to upcast it to `Promise<JsValue>`; `Promise<String>`
would await fine but could not be handed back to JS as a typed promise.

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
pub type SyncKvStorageList<T: ::wasm_bindgen::JsGeneric = JsValue>;

#[wasm_bindgen(method, js_name = "[Symbol.iterator]")]
pub fn iterator<T: ::wasm_bindgen::JsGeneric>(
    this: &SyncKvStorageList<T>,
) -> Iterator<ArrayTuple<(JsString, T)>>;

#[wasm_bindgen(method)]
pub fn list(this: &SyncKvStorage) -> SyncKvStorageList;
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
`<Parent>List<T>` rather than erasing `T`. The method returning it
still has no argument to infer `T` from, so it hands back the wrapper
at its default (see [uninferrable type
parameters](#uninferrable-method-type-parameters)).

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
uses its native ABI representation. Each import states the ABI bounds it
needs, so a generic caller can read them off the declaration: `IntoWasmAbi`
for an argument (`T: RefIntoWasmAbi` for a borrowed `&T`, and
`T: VectorRefIntoWasmAbi` for a borrowed `&[T]`), `FromWasmAbi` for a return,
`Option<T>: IntoWasmAbi`/`Option<T>: FromWasmAbi` for an `Option<T>`,
`NullableIntoWasmAbi` for a written `Nullable<T>` (and under
`--strict-nullable` `NullableFromWasmAbi` for a read one), and `'static`
when awaited. Imported types such as `Holder<T>` need only `'static`:

```rust
#[wasm_bindgen(experimental_generic_mono)]
extern "C" {
    pub fn identity<T>(value: T) -> T
    where
        T: IntoWasmAbi + FromWasmAbi;
    pub async fn load_holder<T>(this: &Store<T>) -> Result<Holder<T>, JsValue>
    where
        T: 'static;
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
that the import it calls states:

```rust
pub fn new<T: ::js_sys::TypedArray>(value: &T) -> TypedArrayOptions
where
    T: ::wasm_bindgen::convert::RefIntoWasmAbi,
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
`Map<JsString, JsString>`, and so on), except for the string leaves of an
argument, below.

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

No variant is generated when the return's only string leaves are inside
built-in containers (a `Promise<Array<string>>` getter or
[`_promise` form](#name_promise--the-un-awaited-form) keeps its single binding),
except for a top-level string list (below), or when a top-level union becomes
a [dynamic-union enum](#erased-return-position-unions-become-dynamic-union-enums):
`"a" | "b"` returns its enum only.

Lists and built-in containers follow the same split between Rust strings and
JS handles. In an argument, every string leaf reached through slices and
built-in containers (`ArrayLike`, `Set`, `Map`, `Promise`, `Iterator`,
`AsyncIterator`) takes `impl JsStringLike`, which wasm-bindgen lowers per
instantiation. A `string[]` accepts `&[String]`, `&[&str]` or `&[JsString]`,
and an empty list needs its element type (`&[] as &[&str]`). Container
handles accept `JsString` elements, and also `String` under
`js_sys_unstable_apis`, where js-sys container elements are real generics
(`Array<String>`, `Set<String>`). A variadic rest array, a constructor's
included, lowers like any other list argument (`...parts: string[]` takes
`&[impl JsStringLike]`, `...values: number[]` takes `&[f64]`), since
wasm-bindgen spreads any sequence. A string list return, sync or awaited,
and its carrier (`Option<Vec<String>>`) get a
`_js_string` variant returning the list as one handle rather than decoding
every element:

```rust
pub fn set_tags(tags: &[impl ::wasm_bindgen::JsStringLike]);
pub fn set_tags_like(tags: &Array<impl ::wasm_bindgen::JsStringLike>);
pub fn set_rows(rows: &[Array<impl ::wasm_bindgen::JsStringLike>]);
pub fn lookup(values: &Map<impl ::wasm_bindgen::JsStringLike, Number>);
#[wasm_bindgen(variadic)]
pub fn join(parts: &[impl ::wasm_bindgen::JsStringLike]) -> String;
pub fn get_tags() -> Vec<String>;
#[wasm_bindgen(js_name = "getTags")]
pub fn get_tags_js_string() -> Array<JsString>;
pub async fn fetch_tags() -> Result<Vec<String>, JsValue>;
#[wasm_bindgen(js_name = "fetchTags")]
pub async fn fetch_tags_js_string() -> Result<Array<JsString>, JsValue>;
```

Async returns use the top-level return lowering in both modes, so this mode
only adds the `_js_string` variant, on the same terms as a sync return:

| TypeScript | Default | `--experimental-generic-mono` |
|---|---|---|
| `Promise<string>` | `Result<String, _>` | `Result<String, _>` + `_js_string` |
| `Promise<string \| undefined>` | `Result<Option<String>, _>` | `Result<Option<String>, _>` + `_js_string` |
| `Promise<Details<string>>` | `Result<Details<JsString>, _>` | `Result<Details<String>, _>` + `_js_string` |
| `Promise<Array<string>>` | `Result<Vec<String>, _>` | `Result<Vec<String>, _>` + `_js_string` |
| `Promise<string[] \| null>` | `Result<Option<Vec<String>>, _>` | `Result<Option<Vec<String>>, _>` + `_js_string` |

The `_js_string` variant of an awaited value resolves the `JsString` form
(`Result<Array<JsString>, _>`, `Result<Option<Array<JsString>>, _>`). The
`_promise` form gets no variant: its string leaves are already inside the
`Promise` handle (`Promise<Array<JsString>>`).

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

### Uninferrable method type parameters

A callable-level parameter that no argument mentions once lowered has
to be named at every call. That is worth it only where the turbofish
is itself the cast — the (awaited) return is exactly `T`, `T |
undefined` or `T | null` — so the rule, per parameter:

1. Mentioned by an argument: kept, inferred at the call. A callback
   counts as it lowers: its parameters and its return count, but a
   callback with a rest parameter is the untyped `Function` and
   mentions nothing. A `Promise<T>` callback return does not count,
   since wasm-bindgen cannot pass a `Function` returning `Promise<T>`
   for a generic `T`. A variadic rest likewise counts as it lowers: by default
   it is `&[JsValue]` and mentions nothing, while under
   `--experimental-generic-mono` a rest array lowers to a slice, so
   `of<T>(...items: T[]): Row<T>` keeps `T` (`fn of<T>(items: &[T]) ->
   Row<T>`) where the default erases it (`fn of(items: &[JsValue]) ->
   Row`).
2. Not mentioned, return exactly `T` / `T | undefined` / `T | null`
   (awaited): kept — `storage.get::<JsString>(key)`, `body.json::<Row>()`.
3. Otherwise erased and removed from the signature. The substitute is
   the default declared by the generic type `T` instantiates in the
   return (`Row<T = Record<string, unknown>>` → `Object`), else the
   method's own default (`list<T = unknown>` → `JsValue`), else `any`.
   A type's default that names that type's own parameters (`U = Box<T>`)
   is skipped, since those names are unbound in the method.
   A parameter the return never mentions is simply dropped.

Callers wanting a typed container `unchecked_into` it
(`D1Result<JsValue>` → `D1Result<Row>`). `try_` companions follow the
same signature. Each overload of a method or free function applies the
rule to its own type parameters, and so does each arm of a
[literal-keyed overload](#inline-wrappers): workers-types'
`get(key, "json")` keeps `ExpectedValue` as its whole return
(`get_json::<Row>(key)`), while `getWithMetadata(key, "json")`
returns `KVNamespaceGetWithMetadataResult` at its defaults.

```ts
interface Store {
  put<T>(key: string, value: T): Promise<void>;
  get<T = unknown>(key: string): Promise<T | undefined>;
  transaction<T>(closure: (tx: Store) => Promise<T>): Promise<T>;
  run<T = unknown>(): Promise<Row<T>>;
  list<T = unknown>(): Promise<Map<string, T>>;
  schedule<Args extends unknown[]>(callback: (...args: Args) => void, ...args: Args): number;
}
```

```rust
pub async fn put<T: JsGeneric>(this: &Store, key: &str, value: &T) -> Result<(), JsValue>;
pub async fn get<T: JsGeneric>(this: &Store, key: &str) -> Result<Option<T>, JsValue>
where
    Option<T>: FromWasmAbi;
pub async fn transaction<T: JsGeneric + FromWasmAbi>(this: &Store, closure: &Function<fn(Store) -> Promise>) -> Result<T, JsValue>;
pub async fn run(this: &Store) -> Result<Row, JsValue>; // Row<Object>, the default
pub async fn list(this: &Store) -> Result<Map<JsString, JsValue>, JsValue>;
pub fn schedule(this: &Store, callback: &Function, args: &[JsValue]) -> f64;
```

### Type-level generics propagate to every binding site

When the surrounding *type* declares parameters
(`interface Foo<T> { … }` / `declare class Foo<T> { … }`), those
parameters thread through every site that mentions `Foo`:

* The `pub type` declaration in the extern block —
  `pub type Foo<T: ::wasm_bindgen::JsGeneric = JsValue>;`. Every
  parameter carries a Rust default: the lowered TS default when one is
  declared (`EventTarget<EventMap = Record<string, Event>>` →
  `= Object<Event>`), otherwise `JsValue`. Upstream declarations use
  type parameters mostly as phantom refinements
  (`Request<CfHostMetadata, Cf>`, `ReadableStream<R>`), and the
  defaults keep the bare name usable everywhere the parameter is
  irrelevant: in `extends = Foo`, in return types of other bindings,
  and in user code. A default lowers like an inner type argument
  (`string` → `JsString`, named types by reference, earlier parameters
  by name); when its target erases to `JsValue` (an unresolved name,
  an alias of an erased union) the whole default is `JsValue`, type
  arguments included. Rust accepts defaults only on the type
  definition, so dictionary builders and helper `impl`s keep the plain
  form and a builder for a defaulted dictionary is named with explicit
  arguments (`Options::<Box_<JsString>>::builder()`, `Box` being in the
  prelude).
* Every reference to the type leaves the trailing arguments that equal
  the declared Rust defaults to them, in both modes. `fetch(input:
  RequestInfo)` instantiates `Request<unknown, CfProperties<unknown>>`,
  which lowers to `Request<JsValue, JsValue>` and, where
  [generic hoisting](#generic-hoisting-for-parameters) leaves it, is
  written `Request`, and `Foo<string, unknown>` with `B = unknown` is `Foo<JsString>`. A
  default naming an earlier parameter is read with that parameter's
  argument, so `Holder<string, Box<string>>` with `U = Box<T>` is
  `Holder<JsString>`. Only the trailing run goes, as Rust can't skip a
  middle argument (`Holder<unknown, Box<string>>` stays
  `Holder<JsValue, Box_<JsString>>`), and the comparison is with the
  Rust spelling, so under `--experimental-generic-mono` a
  `Foo<String>` is kept against a declared `T = JsString`.
* Every getter / setter / method `fn` — redeclares
  `<T: ::wasm_bindgen::JsGeneric>` on the `fn` and uses
  `this: &Foo<T>`. The redeclaration is mandatory because each
  `extern "C"` `pub fn` is independent — the bound on `pub type`
  doesn't propagate to its sibling functions.
* Constructors and static methods have no `this`, so they declare
  only the type-level parameters their own signature mentions. A
  constructor that mentions some returns `Foo<…>` with the mentioned
  parameters by name and the rest at their defaults; one that
  mentions none returns the bare `Foo`, so `Foo::new()` never needs a
  turbofish:
  ```rust
  pub fn new() -> Result<TransformStream, JsValue>;
  pub fn new_with_transformer<I: JsGeneric, O: JsGeneric>(
      transformer: &Transformer<I, O>,
  ) -> Result<TransformStream<I, O>, JsValue>;
  ```
  A default that names an earlier parameter takes that parameter's
  argument, so an unmentioned name never reaches the signature:
  ```ts
  declare class Chained<A = string, B = A, C = number> { constructor(c: C); }
  ```
  ```rust
  pub fn new<C: JsGeneric>(c: &C) -> Result<Chained<JsString, JsString, C>, JsValue>;
  ```
* `extends Foo<X>` lowers to `extends = Foo` — the attribute cannot
  carry generic arguments, and the defaults make the bare head valid.
* A generic type derives `PartialEq, Eq` like any other imported type.
  wasm-bindgen emits both as handle identity with no bound on the type
  parameters (wasm-bindgen#5383), so `Foo<JsValue>`, a subclass of
  `Foo<{…}>` and a child of `Record<string, unknown>` all compare.
* An `async fn` whose resolved value is a bare type parameter bounds
  it `T: JsGeneric + FromWasmAbi` — `JsFuture<T>` converts the
  settled value through `FromWasmAbi`. This holds for methods,
  statics and free functions alike
  (`static load<T>(): Promise<T>` → `pub async fn load<T: JsGeneric +
  FromWasmAbi>() -> Result<T, JsValue>`). Inside a callback type
  (`Function<fn() -> Promise<T>>`) a promised type parameter erases to
  `Promise`, since wasm-bindgen's `JsFunction` cannot normalise the
  nested projection.
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
   Always last. They take the first of `T, U, V, …` not already bound
   in scope, so `GenAbv<T>`'s setter is
   `set_data<T: JsGeneric, U: TypedArray>(this: &GenAbv<T>, val: &U)`.

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
    #[wasm_bindgen(extends = Object, is_type_of = JsValue::is_object)]
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

### Primitive-constrained parameters erase

A type parameter whose constraint is a primitive — `string`, `number`,
`boolean`, `bigint`, a union of literals of one of those, or `keyof X`
(a union of property names, read as `string`) — can only be
instantiated with subtypes that share the primitive's ABI, so a Rust
generic would only ever be `JsString` under another name. The
parameter is dropped from the declaration and reads as the primitive;
references that pass an argument for it drop that argument so the
remaining ones line up:

```ts
interface KVNamespace<Key extends string = string> {
  get(key: Key): Promise<string | null>;
  list<Metadata = unknown>(): Promise<KVNamespaceListResult<Metadata, Key>>;
}
interface Env { TYPED: KVNamespace<"a" | "b">; }
```

```rust
pub type KVNamespace;
pub async fn get(this: &KVNamespace, key: &str) -> Result<Option<String>, JsValue>;
pub async fn list(this: &KVNamespace) -> Result<KVNamespaceListResult, JsValue>;
pub fn typed(this: &Env) -> KVNamespace;
```

(`list`'s own `Metadata` erases separately, as no argument infers it;
see [uninferrable type parameters](#uninferrable-method-type-parameters).)

Method-level parameters follow the same rule
(`tag<Level extends "info" | "warn">(level: Level)` → `level: &str`).

The arguments are dropped once every input is parsed, before codegen and
before [type-level evaluation](#type-level-evaluation), wherever the
reference sits: inside `keyof`, an indexed access, a mapped type or an
inline object literal too, so `Slot<"a", number>["value"]` against
`interface Slot<K extends string, V>` reads `V` as `number`, not `"a"`.
Everything downstream sees `KVNamespaceListResult<Metadata>`. In
per-monomorphization mode, the erased `Key` therefore adds no
`list_js_string` variant.

A type alias's parameters don't erase, even primitive-constrained ones:
evaluation instantiates the alias at its arguments, which reading `K` as
`string` would lose. `Flags<"x" | "y">` against `type Flags<K extends
string> = { [P in K]: boolean }` is a dictionary with `x` and `y`
(`FlagsXOrY`). An alias that evaluation leaves alone lowers to its own
name: `params: Params<P>` against `type Params<P extends string = any> =
Record<P, string | string[]>` is `Params`, with `pub type Params =
Object;`. An object-type alias promoted to an interface (`type Shape<K
extends string> = { key: K }`) is an interface, and erases like one.

### Generic hoisting for parameters

TypeScript accepts any `Request<M, C>` where `Request<unknown,
CfProperties>` is expected, through covariance. The erased Rust form
`&Request<JsValue, JsValue>` would take only that one instantiation,
so a caller holding a `Request<MyMeta, MyCf>` would have to upcast it
and lose its type. So when a parameter's own type is a generic class or
class-like interface and one of its argument positions lowers to the
top type `JsValue` (a default, `unknown`/`any`, an erased union), that
position takes a fresh type parameter on the binding instead:

```ts
declare function fetch(input: RequestInfo | URL, init?: RequestInit<RequestInitCfProperties>): Promise<Response>;
```

```rust
pub async fn fetch<CfHostMetadata: ::wasm_bindgen::JsGeneric, Cf: ::wasm_bindgen::JsGeneric>(
    input: &Request<CfHostMetadata, Cf>,
) -> Result<Response, JsValue>;
#[wasm_bindgen(catch, js_name = "fetch")]
pub async fn fetch_with_str(input: &str) -> Result<Response, JsValue>;
```

Under `--experimental-generic-mono` the fresh parameters are unbounded,
like the owning type's own: each mono position states the ABI bound it
crosses at, and a handle's type arguments cross at none, so JS handles and
native leaves alike are accepted (`Envelope<String, f64>`).

* Free functions, constructors, methods and static methods hoist, sync,
  `async` and `_promise` forms alike. Only parameters do: a return keeps
  the [declared defaults](#type-level-generics-propagate-to-every-binding-site)
  (`fn copy(…) -> Envelope`), as does a setter's value.
* Only the parameter's own type hoists, including each arm a union
  [expands](#signature-flattening) to (`fetch` above, beside
  `fetch_with_str`). Inside `Option<…>` or any other wrapper the
  defaults stay: Rust's fn generics have no defaults, so a caller
  passing `None` would need a turbofish.
* Positions holding anything else keep it (`Envelope<string>` is
  `&Envelope<JsString, Body>`), and a trailing one equal to its default
  is left to it, read against the hoisted parameters.
* A dictionary (and a discriminated union) doesn't hoist. It is built
  through its generated factory, whose type parameters only the
  parameter it is passed to pins, so `f(&Options::new())` keeps
  compiling without an annotation.
* Each fresh parameter is named after the declaration's type parameter,
  numbered (`Meta2`) past the binding's own parameters, a method's
  class parameters included, and past any declared type it would
  shadow (`QueueEvent<Body = unknown>` hoists `Body2` beside the
  `Body` interface). They are declared after the bounds listed above:

```rust
pub fn forward<Meta: JsGeneric, Body: JsGeneric, Meta2: JsGeneric, Body2: JsGeneric>(
    this: &Envelope<Meta, Body>,
    other: &Envelope<Meta2, Body2>,
) -> String;
```

### Primitive union arguments

In per-monomorphization mode, primitive union arguments expand per member
through [signature flattening](#signature-flattening), exactly as without
it, so the names are the same in both modes. Only the argument types
differ: a string member takes `impl JsStringLike`.

```ts
function setValue(value: string | number): void;
function hasKey(key: PropertyKey): boolean;
function send(to: Target | string | number): void;
```

```rust
pub fn set_value(value: impl ::wasm_bindgen::JsStringLike);
#[wasm_bindgen(js_name = "setValue")]
pub fn set_value_with_f64(value: f64);
pub fn has_key(key: impl ::wasm_bindgen::JsStringLike) -> bool;
#[wasm_bindgen(js_name = "hasKey")]
pub fn has_key_with_f64(key: f64) -> bool;
#[wasm_bindgen(js_name = "hasKey")]
pub fn has_key_with_symbol(key: &Symbol) -> bool;
pub fn send(to: &Target);
#[wasm_bindgen(js_name = "send")]
pub fn send_with_str(to: impl ::wasm_bindgen::JsStringLike);
#[wasm_bindgen(js_name = "send")]
pub fn send_with_f64(to: f64);
```

A named alias of the union (`type Id = string | number`) is one
binding instead, taking the alias's own trait (`lookup(id: impl Id)`,
see [named union aliases](#named-union-aliases)). The `js_sys`
primitive-union marker traits (`JsNumberOrStringLike` and the rest) are
not emitted. Return positions are unchanged: an erased
union still becomes a
[dynamic-union enum](#erased-return-position-unions-become-dynamic-union-enums).

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

### `--error-type <path>` — configurable default error

Without `@throws`, fallible bindings default to `Result<T, JsValue>`.
`--error-type <path>` (library: `GenerateOptions::error_type`) replaces
that default with any Rust type path. wasm-bindgen's `catch` lowers to
`take_last_exception()?`, so the type only has to implement
`From<JsValue>`; a project can route every untyped failure into its own
error enum. Bindings whose `@throws` JSDoc names a specific type still
use that type — the flag is purely about the *default*.

```ts
upload(file: File): Promise<void>;
```

emits

```rust
// default
pub async fn upload(this: &Foo, file: &File) -> Result<(), JsValue>;
// with --error-type crate::Error
pub async fn upload(this: &Foo, file: &File) -> Result<(), crate::Error>;
```

The path is emitted verbatim, so it is resolved from the generated
module: use `crate::…` or a fully qualified `::some_crate::…` path.
An unparseable path is a hard error.

`--errors-as-error` is shorthand for `--error-type ::js_sys::Error`,
for environments that prefer the typed `Error` API (`message`,
`name`, `cause`, …) over raw `JsValue`. The trade-off is that
`js_sys::Error: From<JsValue>` is an unchecked cast, so JS code
throwing a non-Error value (`throw "oops"`) yields an `Error` whose
accessors misbehave; if you can't audit your sources for that, stay
with `JsValue` or use an enum with a checked `From<JsValue>`.

### `@throws {X}` on getters

Getters and setters are infallible: property access in JS does not
normally throw, and a `Result` on every field read would be noise. A
getter annotated `@throws {X}` is the exception. It emits `getter,
catch` returning `Result<T, X>`, for instance and static getters alike.
This applies with or without `--fallible`: there is no `try_` form for a
getter, so an explicit annotation has only one possible lowering.
Setters never catch.

```ts
declare class Body {
  /** @throws {RangeError} when the body was already consumed */
  readonly consumedText: string;
}
```

emits

```rust
#[wasm_bindgen(method, getter, js_name = "consumedText", catch)]
pub fn consumed_text(this: &Body) -> Result<String, RangeError>;
```

### `--error-enum [NAME]` — `@throws` types become one enum

Without the flag, `@throws {TypeError | RangeError}` lowers through the
[subtyping LUB](#subtyping-lub-across-unions) to `Result<T, Error>`, and
the caller can no longer tell the two apart without `dyn_ref`. The
`js_sys` error classes' `From<JsValue>` are unchecked casts, so no
single `js_sys` type can carry a union faithfully. `--error-enum`
(library: `GenerateOptions::error_enum(name)`; the CLI default name is
`JsException`) instead collects every type named in any `@throws` across
the output — union members flattened — and emits one enum:

```rust
#[derive(Debug, Clone)]
pub enum JsException {
    QuotaError(QuotaError),
    TypeError(TypeError),
    RangeError(RangeError),
    DOMException(DOMException),
    Error(Error),
    Other(JsValue),
}
```

with `From<JsValue>` (a chain of `dyn_into` checks ending in `Other`),
`From<JsException> for JsValue`, `AsRef<JsValue>`, `Display` and
`std::error::Error`. `Display` is the `message` of an `Error` instance;
for any other throwable it is `String(value)` (`42`, `null`,
`[object Object]`), falling back to `Debug` if that conversion throws
(an object without `toString`). It never panics.

Rules:

* Variants are ordered **most derived first**: a class precedes every
  ancestor in its chain (builtin table plus user `extends`), deeper
  chains go first, ties keep source order. A class whose ancestry is
  not visible (an `--external` mapping with no `extends` in the input)
  goes before every builtin-rooted class, since an external error class
  almost certainly extends `Error` and the `Error` arm would otherwise
  swallow it. Declare `extends` on such a class to place it exactly.
* Every binding with a `@throws` type returns `Result<T, JsException>`:
  unions and single types alike. A single type could keep its precise
  `Result<T, TypeError>`, but then a `?`-style caller needs a `From`
  per class; one enum everywhere means one `?`-able type, and the
  variant still identifies the class. The trade-off is that the static
  signature no longer says which classes a given call can throw — the
  rendered `## Errors` doc section still does.
* Bindings without a `@throws` type keep the `--error-type` default
  (or `JsValue`). A project that wants a single error type overall
  wraps both: `enum Error { Js(JsException), … }` with `From<JsException>` and
  `From<JsValue>`, and passes `--error-type crate::Error`.
* A `@throws` name resolves like a type reference from the declaration
  carrying it: `{Fault}` inside `namespace Ns` and `{Ns.Fault}` outside name
  the same class and share one variant. A declared class's variant is
  named by its Rust path, so `{A.Fault | B.Fault}` gives `AFault(a::Fault)`
  and `BFault(b::Fault)`.
* `Other` is reserved for the fallback arm, and an ident already taken
  is numbered in source order, first seen wins: a declared `class Other`
  is `Other2(Other)`, and `{AFault | A.Fault}` gives `AFault(AFault)` and
  `AFault2(a::Fault)`.
* A `@throws` name that is an interface (`interface AiError extends Error
  {}`) has no runtime class to test a thrown value against: its
  `dyn_into` accepts [any object](#shapes-have-no-runtime-class), so its
  arm would swallow every error. It is reported and omitted from the
  enum; its values land in another variant (`Error`, if one is named) or
  `Other`.
* A `@throws` name must be declared in the input, a `js_sys` error
  class (`Error`, `TypeError`, `RangeError`, `SyntaxError`,
  `ReferenceError`, `EvalError`, `UriError`), or mapped with
  `--external`. Anything else is an error diagnostic and is omitted
  from the enum; values of that class land in `Other`.
* The enum is emitted whenever the flag is given, even when no
  `@throws` exists, so downstream code can reference it
  unconditionally. A name that collides with a declared type is an
  error. The default is `JsException` rather than `JsError` because
  `wasm_bindgen::prelude::*` already exports a `JsError`, and a file
  that globs both the bindings and the prelude would find the name
  ambiguous.

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
  binding — no `try_<name>` companion is emitted. Under `--fallible`
  the primary drops its `catch` and returns `T` directly.
* **Async callables** (returning `Promise<T>`) drop the `Result<T, _>`
  wrapper and the `catch` attribute. The binding becomes
  `pub async fn foo() -> T`; its `foo_promise` form is plain under
  every policy, as a builtin's, a global's or an `async` callable's is
  [without the annotation](#builtins-globals-and-async-never-throw-synchronously).
* **Constructors** always catch per JS `new` semantics —
  `@throws {never}` on a constructor is silently ignored.
* **`never` returns** contradict the annotation and win: it is ignored
  and the callable keeps only its catching form (see
  [`never` returns](#never-returns-keep-only-the-catching-form)).
* **Setters** never throw and never get `try_` variants regardless,
  so the annotation is a no-op there too.
* **Mixed annotations** like `@throws {never | TypeError}` collapse to
  `@throws {TypeError}` (since `T | never` is just `T` in TS) — the
  `never` arm is absorbed and the residual type drives codegen as
  usual.
* **Rendered doc**: `@throws {never}` lines do *not* contribute to
  the `## Errors` section. By definition there are no errors to
  document.

## Inline wrappers

Everything ts-gen emits is an `extern "C"` binding, except where the
extern ABI cannot express the declared type. There, the extern binding
is emitted under a private name and a non-extern `pub fn` of the public
name converts over it. The rule is narrow on purpose: a wrapper is
never emitted for ergonomics, only where no extern signature can carry
the type, and the extern form stays reachable (within the module) under
a `__`-prefixed name (`__<name>`). Three constructs qualify today:

* **`null` arms.** The extern ABI has no type that only admits `null`,
  so `f(x: T | null)`'s `f_with_null()` binds `__f_with_null(x:
  &JsValue)` and passes `&JsValue::NULL` (see [Writes](#writes)).
* **Named union alias arguments.** An extern argument cannot be
  `impl Into<T>`, so `f(x: PlainDateLike)` binds `__f(x: PlainDateLike)`
  and the wrapper converts its `impl Into<PlainDateLike>` argument (see
  [named union aliases](#named-union-aliases)). A literal or `null`
  arm's wrapper converts the same way. Under
  `--experimental-generic-mono` the extern takes the alias's trait,
  `impl PlainDateLike`, itself, and needs no wrapper.
* **Overloads keyed by a string literal**, below.

**Overloads distinguished only by a string literal.** Rust has no
literal types, so `get(key, type: "json")` and `get(key, type:
"arrayBuffer")` both lower to `(key: &str, type: &str)` and used to
collapse into one binding with the first overload's return type. The
extern ABI cannot carry the literal; the wrapper can:

```ts
interface KVNamespace {
  get(key: string): Promise<string | null>;
  get(key: string, type: "text"): Promise<string | null>;
  get(key: string, type: "json"): Promise<any>;
  get(key: string, type: "arrayBuffer"): Promise<ArrayBuffer | null>;
  get(key: string[], type: "text"): Promise<Map<string, string | null>>;
  get(key: string[], type: "json"): Promise<Map<string, any>>;
}
declare class CompressionStream {
  constructor(format: "gzip" | "deflate" | "deflate-raw");
}
```

```rust
extern "C" {
    pub async fn get(this: &KVNamespace, key: &str) -> Result<Option<String>, JsValue>;
    #[wasm_bindgen(method, catch, js_name = "get")]
    async fn __get_text(this: &KVNamespace, key: &str, r#type: &str) -> Result<Option<String>, JsValue>;
    #[wasm_bindgen(method, catch, js_name = "get")]
    async fn __get_json(this: &KVNamespace, key: &str, r#type: &str) -> Result<JsValue, JsValue>;
    #[wasm_bindgen(method, catch, js_name = "get")]
    async fn __get_array_buffer(this: &KVNamespace, key: &str, r#type: &str) -> Result<Option<ArrayBuffer>, JsValue>;
    // …
    #[wasm_bindgen(constructor, catch)]
    fn __new(format: &str) -> Result<CompressionStream, JsValue>;
}
impl KVNamespace {
    pub async fn get_text(&self, key: &str) -> Result<Option<String>, JsValue> {
        self.__get_text(key, "text").await
    }
    pub async fn get_json(&self, key: &str) -> Result<JsValue, JsValue> { /* "json" */ }
    pub async fn get_array_buffer(&self, key: &str) -> Result<Option<ArrayBuffer>, JsValue> { /* … */ }
    pub async fn get_text_with_slice(&self, key: &[String]) -> Result<Map<JsString, JsNullable<JsString>>, JsValue> { /* … */ }
    pub async fn get_json_with_slice(&self, key: &[String]) -> Result<Map<JsString, JsValue>, JsValue> { /* … */ }
}
impl CompressionStream {
    pub fn new_gzip() -> Result<CompressionStream, JsValue> { Self::__new("gzip") }
    pub fn new_deflate() -> Result<CompressionStream, JsValue> { Self::__new("deflate") }
    pub fn new_deflate_raw() -> Result<CompressionStream, JsValue> { /* "deflate-raw" */ }
}
```

* A **cohort** is a set of two or more expansions that agree on every
  parameter except string-literal ones at the same positions. The arms
  come from separate overloads or from one overload's literal union
  (`"gzip" | "deflate" | "deflate-raw"`); a lone literal
  (`getAll(name: "Set-Cookie")`) is not a cohort and keeps the plain
  `&str` binding, as does a cohort that also absorbed a plain `string`
  arm (`"a" | "b" | string`), since the plain form already covers it.
* A literal repeated across overloads (`tag(m: "a" | "b")` beside
  `tag(m: "b" | "c")`, or a merged declaration's copy of an overload)
  is one arm: `tag_a`, `tag_b`, `tag_c`. It keeps the first overload's
  return type, as TypeScript resolves a call to the first overload that
  matches.
* Each wrapper is named `<base>_<literal>` in snake case, followed by
  the `_with_` suffix the remaining parameters earn among the plain
  expansions and the other cohorts' remaining parameters (`get_json`,
  `get_json_with_slice`). An arm left with no
  parameter to name earns none, since the literal already sets it apart:
  Temporal's `getTimeZoneTransition(direction: "next" | "previous")`
  beside `(direction: TransitionOptions)` gives
  `get_time_zone_transition_next` and `get_time_zone_transition(&self,
  direction: &TransitionOptions)`. Every arm is named
  explicitly; no arm takes the bare base name, which stays with the
  literal-free overload (`get(key)`) when there is one.
* Arms with the same return type share one extern binding; a
  different return type gets its own, since the extern return is what
  wasm-bindgen converts. A binding serving every arm of the cohort is
  named for the cohort behind a `__` prefix (`__new` serves all three formats,
  `__get_time_zone_transition` both directions). When the arms need
  several, each is named after its first arm instead (`__get_text`,
  `__get_json`), since the cohort's name alone cannot tell them apart.
* The `_promise` and `try_` companions follow per arm
  (`get_json_promise`, `try_describe_raw`) over their own extern
  (`__get_json_promise`, `__try_describe`), and so, under
  `--experimental-generic-mono`, does the `_js_string` return variant
  (`describe_raw_js_string` over `__describe_js_string`). The prefix,
  rather than a suffix, keeps a private binding from reading as one of
  these public variants. A cohort's binding steps aside for a plain
  `null` arm's (`__<name>_2`): `f(x: T | null)` beside `f(kind: "a" |
  "b", x: T | null)` gives `f_with_null()` over `__f_with_null` and
  `f_a_with_null()` over `__f_with_null_2`.
* A plain `null` arm the literal arms leave alone keeps `_with_null`
  (see [Writes](#writes)): `mode(kind: "x" | "y" | null)` gives `mode_x()`,
  `mode_y()` and `mode_with_null()`, not a bare `mode()`.
* A generic receiver (`KVStore<Key>`) gives `impl<Key: JsGeneric>
  KVStore<Key>`; the method's own type parameters stay on the wrapper
  fn. A static's wrappers go where wasm-bindgen puts the static: one
  returning its own type hoists those type arguments
  (`impl<T> Boxed<T> { pub fn of_a(value: &T) -> Boxed<T> }`), a bare one
  binds the type's defaults (`impl Boxed { pub fn label_a() -> String }`). A wrapper repeats its extern's `where` clause, since it calls the
  extern under the same bounds: the ABI traits a generic import states
  under `--experimental-generic-mono` (`Key: IntoWasmAbi`), and an
  awaited carrier's (`Option<ExpectedValue>: FromWasmAbi` on
  `get_json`).
* A `null` arm beside the literal (`pick(kind: "a" | "b", x: T |
  null)`) makes a cohort of its own, named `_with_null` as a lone
  `null` arm is. Its wrappers drop the `null` parameter too and pass
  `&JsValue::NULL`, over one `__pick_with_null` extern taking
  `&JsValue` there; a type parameter only that position named is
  dropped from both:

  ```rust
  extern "C" {
      fn __pick(this: &Picker, kind: &str, x: &ArrayBuffer);
      fn __pick_with_null(this: &Picker, kind: &str, x: &JsValue);
  }
  impl Picker {
      pub fn pick_a(&self, x: &ArrayBuffer) { self.__pick("a", x) }
      pub fn pick_a_with_null(&self) { self.__pick_with_null("a", &JsValue::NULL) }
      // … `pick_b`, `pick_b_with_null`
  }
  ```
* Doc comments move to the wrappers; the extern binding is not `pub`.
* Setters are exempt: `set_disposition("inline")` takes the value. A
  setter taking a named union alias is wrapped like any binding
  (`set_relative_to(&self, val: impl Into<ZonedDateTimeLike>)`).

## `@rust` JSDoc → Rust scalar

TS `number` lowers to `f64`, which is wrong-shaped for most of the
numbers a platform API actually carries (`status: number` is a `u16`,
`port: number` a `u16`, `limit: number` a `u32`). A `@rust` tag forces
the Rust scalar; wasm-bindgen converts the JS number at the boundary:

```ts
interface Response {
  /** @rust u16 */
  readonly status: number;
  /**
   * @rust status: u16
   * @rust u32
   */
  withStatus(status: number, reason?: string): number;
  /** @rust u32 */
  count(): Promise<number>;
}
```

```rust
pub fn status(this: &Response) -> u16;
pub fn with_status(this: &Response, status: u16) -> u32;
pub async fn count(this: &Response) -> Result<u32, JsValue>;
```

* `@rust <type>` applies to the member's own type: a property (getter,
  setter and builder method alike) or a callable's return.
* `@rust <param>: <type>` applies to the named parameter of a method,
  constructor or function; several tags may be stacked.
* The scalar replaces the leaf through `| null` / `| undefined` and an
  optional declaration (read as `Nullable<u16>` / `Option<u16>`), `T[]`
  (`Vec<u8>` / `&[u8]`) and an async return's `Promise<…>`
  (`Result<u32, _>`).
* A generic handle's argument must be a JS type, so inside one the
  tagged leaf keeps the JS number it narrows: `Number`, or `BigInt` for
  `u64`/`i64`. A `Promise<number>` getter or argument is
  `Promise<Number>`, `number[][]` is `Vec<Array<Number>>` and
  `(number | null)[]` is `Vec<JsNullable<Number>>`; the tag still
  applies where the leaf reaches a Rust position.
* Accepted scalars: `u8 u16 u32 u64 i8 i16 i32 i64 f32 f64 usize`.
  wasm-bindgen crosses `u64`/`i64` as a JS `BigInt`, so they apply only
  to a `bigint` leaf; every other scalar applies only to a `number`
  leaf. A `@rust u64` on a `number` would make the JS glue reject the
  value at runtime, so it is refused: sizes beyond `u32` stay `f64`
  (exact to 2^53). An unknown scalar, an incompatible leaf, or a
  parameter name that does not exist is an error diagnostic and the
  tag is ignored.

Why a JSDoc tag rather than a branded alias (`type u16 = number`): the
alias form needs the upstream declaration to *use* the alias, which an
upstream file never does, so annotations would still have to be
layered on — and a tag reads as what it is, a Rust-side refinement,
while a `u16` alias in `.d.ts` suggests a TS-level distinction that
does not exist.

## `--annotate` side files

Upstream declaration files (`@cloudflare/workers-types`,
`lib.dom.d.ts`) carry no `@rust` or `@throws` tags, and editing a
vendored copy does not survive an upgrade. `--annotate` takes side
`.d.ts` files that re-declare **only the members they tag**; the tags
are applied to the input's declarations by declaration name and member
name, and the side file is otherwise not emitted:

```ts
// annotations.d.ts
interface Response {
  /** @rust u16 */
  readonly status: number;
  /** @throws {TypeError} if the body has already been used */
  clone(): Response;
}
interface Console { /** @throws {never} */ log(...data: any[]): void; }
declare abstract class Cache { /** @rust ttl: u32 */ put(url: string, ttl?: number): Promise<void>; }
```

```sh
ts-gen --input node_modules/@cloudflare/workers-types/index.d.ts \
       --annotate annotations.d.ts …
```

* `@rust` applies as if written on the input member; a property's tag
  reaches its setter too. Parameters match by name.
* `@throws` replaces the input member's failure mode. Getters are
  infallible, so on a property it contributes only its doc.
* A side member applies to every overload of the input method or
  function, and its rendered doc (the `## Errors` section of `@throws`)
  is appended to the input's doc once. Overloads may differ in return
  type (`delete(key): Promise<boolean>`, `delete(keys: string[]):
  Promise<number>`), so a return `@rust` applies to the overloads that
  can carry it.
* A declaration, member or parameter named in the side file that does
  not exist in the input fails the run, so a typo cannot silently miss
  a binding; so does an invalid `@rust` tag, or one whose scalar the
  input's type cannot carry. Side members without tags are ignored.
* Only classes, interfaces and functions can be annotated: a
  `declare namespace`, `declare module`, alias or variable in the side
  file fails the run rather than being skipped, as does a bare name
  that the input declares more than once (globally and inside a
  module), since names are matched without scope.
* A side declaration's type-parameter defaults replace the input's, by
  parameter name; a default for a parameter the input lacks fails the
  run. The output changes where a default is read, which is an alias's
  expansion. The `annotate` fixture's `RequestInfo<Meta = unknown, Cf =
  CfProperties> = Request<Meta, Cf> | string`, modelled on
  workers-types, defaults `Cf` to a union of the incoming
  and outgoing shapes (`CfProperties = Incoming | Outgoing`), which
  erases, so a `RequestInfo` argument lowers to `&Request<JsValue,
  JsValue>`; the side file's `type RequestInfo<Meta = unknown, Cf =
  Incoming> = …` (whose target is ignored) makes it `&Request<JsValue,
  Incoming>`. A class or interface default is replaced as well, but the
  emitted `Request<Meta, Cf>` has no Rust default, so a bare `Request`
  is unchanged.
* The side file may use `interface` for a class and need not repeat
  type parameters or parameter types other than tagged ones; only names
  matter.

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
[`--error-enum`](#--error-enum-name--throws-types-become-one-enum) opts
`@throws` unions out of the LUB in favour of a generated enum.

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
does; in a return position that does not hoist (a free function, a
property; a method's `A & { … }` return is hoisted, see
[Method return position](#method-return-position)) the literal's extra
fields are not reachable. Neither is unsound at the ABI.

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

#### Shape arms go last

The tuple arms dispatch in order through `dyn_into`, and a
[shape](#shapes-have-no-runtime-class) or `Object` arm (`object`,
`Record<K, V>`, an inline object type) accepts any object, so it would take the values of every arm after it. Such an arm
is moved after the others, before a trailing `JsValue` catch-all. One
shape arm keeps its type and, as the last tuple arm, makes the enum
`#[wasm_bindgen(fallback)]`, which wasm-bindgen recommends for an arm
with no runtime witness: whatever the earlier arms don't take lands
there. Two or more shape arms can't be told apart at runtime, so they
collapse into one `Object(Object)` arm, with a warning. A union left
with only that arm [erases](#arms-a-read-can-tell-apart) instead.

```ts
declare function pick(): RoundingOptions | Ticket;
declare function pickAny(): RoundingOptions | Init | Ticket;
```

```rust
#[wasm_bindgen(private, fallback)]
pub enum PickReturnKind {
    Ticket(Ticket),
    RoundingOptions(RoundingOptions),
}
#[wasm_bindgen(private, fallback)]
pub enum PickAnyReturnKind {
    Ticket(Ticket),
    Object(Object),
}
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

Nested unions inside generics (e.g. `Array<32 | "foo">`, a
`_promise` form's `Promise<32 | "foo">`) still go through the
inner-position lowering — the synthesised-enum path doesn't reach into
generic containers because wasm-bindgen requires `T: JsGeneric` for
`Promise<T>` / `Map<K, V>` / etc. and a synthesised enum doesn't
qualify. The awaited value of an `async fn` is a top-level return and
does synthesise (`fetchValue(): Promise<32 | "foo">` →
`Result<FetchValueReturnKind, E>`).

#### Union aliases flatten into the read

A read of a [named union alias](#named-union-aliases) whose arms it can
tell apart is the alias's own enum. Otherwise a read resolves each union
alias it names, at the top level or as a union member, to the alias's
members, so nested aliases are one union in normal form, each member
once. An alias whose members would read
differently at the use site (a namespace shadowing one of them) stays
whole, as it does for [writes](#type-aliases-and-export--x-as-y-). A
read of `x: Mode | Foo` with `type Mode = 0 | 1` is the same read as
`x: 0 | 1 | Foo`.

#### Arms a read can tell apart

wasm-bindgen dispatches a read by trying each tuple arm's runtime check
in order (`instanceof` for a class, `typeof` for a primitive), so the
members become arms it can decide between:

* Members lowering to the same Rust payload are one arm, the first.
* Shapes and `Object` handles (`object`, `Record<K, V>`, an inline
  object type) accept any object, so they are one arm, after the others,
  as [Shape arms go last](#shape-arms-go-last) describes: one keeps its
  type, several share an `Object` arm, with a warning, and as the last
  arm it is the enum's `#[wasm_bindgen(fallback)]`.
* A member lowering to `JsValue` takes anything, so it is one arm, last.
  The shape arm before it keeps its `is_object` check and isn't the
  fallback.
* A union left with a single arm can't be told apart and keeps its
  regular lowering, so a union of dictionaries reads as `JsValue`
  (`Option<JsValue>` for an optional property), with no warning.

```ts
declare namespace Temporal {
  type PlainDateLike = PlainDate | ZonedDateTime | PlainDateTime | DateLikeObject | string;
  type ZonedDateTimeLike = ZonedDateTime | ZonedDateTimeLikeObject | string;
  type TimeZoneLike = ZonedDateTime | string;
  interface DurationRelativeToOptions {
    relativeTo?: ZonedDateTimeLike | PlainDateLike | undefined;
  }
  interface InstantToStringOptions {
    timeZone?: TimeZoneLike | undefined;
  }
}
```

```rust
pub fn relative_to(this: &DurationRelativeToOptions) -> Option<RelativeToKind>;
// The alias read whole is its own enum.
pub fn time_zone(this: &InstantToStringOptions) -> Option<TimeZoneLike>;

#[wasm_bindgen(private, fallback)]
pub enum RelativeToKind {
    ZonedDateTime(temporal::ZonedDateTime),
    String(String),
    PlainDate(temporal::PlainDate),
    PlainDateTime(temporal::PlainDateTime),
    Object(Object),
}
```

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

As in TypeScript's binder, an ambient module is one symbol with one
exports table: everything a `declare module "x"` block exports merges
into the module, from any input file and also from a module
augmentation (`import "x"; declare module "x" { … }` in a module file).
An interface declared in two blocks is one type with the members of
both, a function's declarations are its overloads, and a namespace's
members are one module. A block's imports and, in a block with an
export declaration (`export { … }`, `export =`), its unexported
declarations are its own: they merge with no other block's, and a name
in the block resolves through them first, then through the module's
exports, its file and the globals. A block without an export
declaration exports every declaration; one with an export declaration
exports those with an `export` modifier and those it names. Each
`namespace N { … }` declaration is bound the same way: what it exports
is a member of the namespace, shared with its other declarations in any
file, and the rest is its own.

A name in a merged declaration resolves from the declaration it is
written in, as in TypeScript: each method, property, construct
signature and `extends` clause from its own interface or class body,
and each namespace member from its own `namespace` block. Two files'
`declare module "x"` blocks importing different `T`s each keep theirs
in one merged interface, and with `import { T as Ev } from "ia"; declare
global { interface Win { b: Ev } }` in a module file, `b` is `ia`'s `T`
while a script's `interface Win { a: Ev }` keeps the global `Ev` for
`a`.

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
   in neither `--export` nor `--external`. Its exports are not emitted,
   but a type an emitted signature reaches through an import is, as a
   type only (see [Global scope](#global-scope)). A reference that
   doesn't import the module's declaration doesn't see it at all (see
   [The export map](#the-export-map)).

`--external` always wins over `--export`: a module in both is
externalised, not emitted.

For `Global` declarations (top-level `declare class`, `declare
function`, `declare const`, etc. in a script file) the rule is simpler:
a file's globals are emitted when the file is in `--export`.

A file with any top-level `import` or `export` is a module, and with
`--lib-name` every top-level declaration in it is `Module("<lib>")`,
with or without an `export` prefix. An unexported `interface Options`
or `declare var _Foo` is still exported by name through
`export { _Foo as Foo }`, or referred to by the exported declarations,
and either way it merges and resolves with them rather than being a
global the output never emits. As in TypeScript, `import x =
require("m")` is such an import, but `import x = N.M`, which aliases an
entity, makes a file a module only as `export import x = N.M`.

### The export map

Each block (file, `declare module` body, namespace body) exports every
declaration unless it contains `export { … }`, `export … from`,
`export *`, `export =` or `export default <expr>`; then only what those
and `export` modifiers name. `export default class C {}` is a modifier.
`export = X` adds the exports of the namespace merged with `X` as named
exports. Per block, not per merged scope:

```ts
namespace test { export { test }; }        // exports: test
namespace test { function after(): void; } // exports: after
```

Imports look up public names and bind the local: `export { _Foo as
Foo }` makes `import { Foo }`, `D.Foo` and `extends Foo` all reach
`_Foo`, and the Rust reference is `_Foo`.

The output is built from export maps: each export of a selected file or
module is emitted under its public name, a function or variable once
per name and a type under its local name with an alias per public name
(see [Type aliases and `export { X as Y }`](#type-aliases-and-export--x-as-y-)).
A default export takes its local's Rust name, or `default` when a named
export of the same declaration has it: `export { f, f as default }`
binds `f()` and `default()`. An `export default` declaration's local
name is not exported, also in a block without `export { … }`, so
`export default function f()` binds only `default`, as `f()`.

Qualification keys off the *resolved* declaration's `module_context`,
not the textual name, so a global `interface Foo` and a module-scoped
`class Foo` resolve independently — scope-chain resolution at the use
site picks the visible one. Each module body is a child scope of its
file, as in TypeScript: its declarations see the file's globals, but
are not visible to the file or to other modules, and repeated
`declare module "<spec>"` blocks share only what the module exports.
Another file or module reaches them with `import { Foo } from
"<spec>"`, which resolves against the module's scopes before looking
for a file.

A name resolves only from where it is written, as in TypeScript: through
the enclosing declarations, blocks and file, then the global scope. A
declaration the use site neither sees nor imports is not found, even
when nothing else has its name and another file or module declares it:
a module file's or `declare module` block's unexported declarations, or
a module's exports from a script. TypeScript reports "Cannot find name";
ts-gen reports an error and binds the name like any other unknown one,
through the external map or a `use JsValue as Name;` alias, so the output
still compiles:

```ts
// a.d.ts, a module file with --lib-name
interface Shared { fromA(): void; }
export declare function a(): void;
export {};
// main.d.ts
export declare function useShared(s: Shared): void;
```

```text
error: Unresolved type `Shared`. Use --external to map this type.
```

```rust
use JsValue as Shared;
pub fn use_shared(s: &Shared);
```

So a second file's `interface Shared` doesn't make the reference depend
on which file comes first.

A file is looked for from the directory of the file the import is
written in, also for an import in a `declare module` block, so the
output does not depend on where ts-gen runs: `import { P } from "pkg"`
in a block finds the `pkg` in the `node_modules` beside its file. As in
TypeScript, two kinds of import are not resolved, each reported with
TypeScript's message: a relative name such as `"./dep"` in a script's
`declare module` block (TS2439), and any import or re-export in a module
augmentation (TS2667, TS2666), whose imports belong at the top of its
file. A
name such an import would bind resolves as if it were absent.

```ts
// a.d.ts, a script
declare module "lib" {
  import { P } from "pkg";   // a's directory's node_modules/pkg
  import { R } from "./rel"; // warning, not resolved
}
// b.d.ts, a module
import { R } from "./rel";   // b's directory's rel.d.ts, seen by the block
declare module "lib" {
  import { Q } from "pkg";   // warning, not resolved
}
```

### Global scope

As in TypeScript, there is one global scope. A script file's top-level
declarations and every `declare global` block declare into it, after the
built-in types, so declarations of a name in different files merge into
one, as they do within a file:

```ts
// a.d.ts
interface Win { a(): void; }
// b.d.ts
interface Win { b(): void; }
```

```rust
pub type Win;
pub fn a(this: &Win);
pub fn b(this: &Win);
```

That includes the split TypeScript's own libs use, an interface in one
file and its constructor `var` in another (`declare var X: { new(): X;
… }` or `declare var X: XConstructor`): in either order they are one
class, as within a file. Every file's `interface XConstructor` blocks
are the static side of that class and are not emitted as a type, so
`from` and `of`, which lib.es2015.core adds to `ArrayConstructor`, are
statics of `Array`. Its construct signatures, from every file in file
order, are the class's constructor overloads, which expand as `new`,
`new_with_…` like any overloads. A value never takes a type's
name, so `interface F` and `declare function F(): F` in different files
keep `F` the interface.

A file without `--lib-name` is read as a script, its globals are its
exports, and selecting it emits each of them whole, members from other
files included. Without `--export`, every input file is selected.

What a selected export's signatures reach and no selected export
provides is emitted as a type only: an interface, alias or enum as is,
a class without its constructor and statics, and a namespace only as
the `pub mod` path to what is emitted. A function or variable is
emitted only as an export. With `--export b.d.ts`, a function in
`b.d.ts` taking `OnlyA` from `a.d.ts` emits `OnlyA` but not `a.d.ts`'s
functions. A type that no selected export provides and no emitted
signature reaches is not emitted, so an interface a module with `export
{ … }` doesn't export and none of its exports use is left out.

### Output surface: `--export` and `--input`

`--input` defines the *type universe* — every file gets parsed so
resolution sees the full set. `--export` defines the *output surface* —
what actually gets emitted as public Rust code. Three forms:

* **File path** (e.g. `--export types/email.d.ts`) — emit the globals
  that file declares.
* **Module specifier** (e.g. `--export cloudflare:email`) — emit that
  module's declarations in its `pub mod`, attribute-attached.
* **Declaration name** (e.g. `--export Request`, `--export
  TailStream.TailEvent`) — emit that export of any file or module, and
  the types its bindings reach. See [Root selection](#root-selection).

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

### Root selection

A large upstream declaration file (`@cloudflare/workers-types`,
`lib.dom.d.ts`) is a single file, so file-level `--export` is
all-or-nothing. Naming declarations keeps the whole file as the type
universe while emitting only what a chosen surface needs:

```sh
ts-gen --input node_modules/@cloudflare/workers-types/index.d.ts \
       --export Request --export Response --export KVNamespace
```

`--export Request` selects the export `Request` of every root, each
file's globals and each module not externalised, as if that root were
selected with only that export in it. A dotted name goes through a
namespace's exports: `--export TailStream.TailEvent` emits `TailEvent`,
with `tail_stream` only the `pub mod` path to it. Functions and
variables are named by their export, the source spelling (`--export
setTimeout`).

What the selected exports reach is emitted as a type only, as for any
selection (see [Global scope](#global-scope)); a module-context
declaration is emitted in its module's `pub mod` with its
`#[wasm_bindgen(module = "...")]` attribute. Files and modules listed
alongside are selected whole.

A bare dotted identifier is read as a declaration name unless it names
a `declare module`; a name no root exports is a codegen error.

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

declare module "cloudflare:workers" {
  import { EmailMessage } from "cloudflare:email";
  export class ForwardableEmailMessage extends EmailMessage { /* ... */ }
  export interface SendEmail {
    send(message: EmailMessage): Promise<void>;
  }
}
```

With `--export cloudflare:email --export cloudflare:workers`:

```rust
pub mod cloudflare {
    pub mod email {
        #[wasm_bindgen(module = "cloudflare:email")]
        extern "C" {
            pub type EmailMessage;
        }
    }
    pub mod workers {
        #[wasm_bindgen(module = "cloudflare:workers")]
        extern "C" {
            #[wasm_bindgen(extends = super::email::EmailMessage, extends = Object)]
            pub type ForwardableEmailMessage;
        }
        #[wasm_bindgen(module = "cloudflare:workers")]
        extern "C" {
            pub type SendEmail;
            #[wasm_bindgen(method, catch)]
            pub async fn send(this: &SendEmail, message: &super::email::EmailMessage)
                -> Result<(), JsValue>;
        }
    }
}
```

### `namespace` is unrelated to JS modules

```ts
namespace WebAssembly {
  class Module { ... }
}
```

emits a `pub mod web_assembly { ... }` (note: still `pub`) with
`#[wasm_bindgen(js_namespace = "WebAssembly")]` on each member that
names something in JS (an interface's type, which
[names nothing](#shapes-have-no-runtime-class), and a string enum don't
carry it).
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
#[wasm_bindgen(extends = events::Emitter, extends = Object, is_type_of = JsValue::is_object)]
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
way. The assigned namespace may also be declared outside the module, as
`workers-types` does with `declare namespace CloudflareWorkersModule`
and `declare module "cloudflare:workers" { export = CloudflareWorkersModule; }`.
It is then the module's rather than a global, so its members are emitted
in the module's `pub mod` with `module = "cloudflare:workers"`, imports
from the module resolve to them, and global references such as
`CloudflareWorkersModule.WorkerEntrypoint` qualify to that `pub mod`.
Emitting them takes `--export cloudflare:workers`.

This needs the namespace to be the global the name binds as a value, and
first declared in the module's file; a later file reopening it adds to
the module. A binding of the module's own with that name is what it
assigns instead. A global function or class merged with the namespace,
a namespace an earlier file declares, a second module assigning the same
namespace, and any other `export =` of a global are skipped with a
diagnostic.

A namespace merged with a same-named class, function or interface
(`class Foo {}` + `namespace Foo { interface Options {} }`) leaves
that declaration as the type `Foo`, while `Foo.Options` resolves
through the namespace, in either declaration order.

In a module, the namespace's members bind through the name the module
exports it under, which also names its Rust module. That differs from the
local name for a value merged with the namespace and exported renamed or
as the default:

```ts
declare module "m" {
  function make(): void;
  namespace make { function helper(): void; }
  export { make as create };
}
```

```rust
pub mod m {
    // …
    pub fn create();
    pub mod create {
        #[wasm_bindgen(module = "m")]
        extern "C" {
            #[wasm_bindgen(js_namespace = "create")]
            pub fn helper();
        }
    }
}
```

With `export default make`, the members take `js_namespace = "default"`
and the module keeps the local name, as `default` is no name of its own. A
namespace exported under its own name as well keeps it.

## Type aliases and `export { X as Y }`

* `type Foo = Bar;` → `pub type Foo = Bar;` if `Bar` is a recognised
  type, or chases the alias chain to its terminal during codegen. An
  alias inside a namespace is emitted in the namespace's `pub mod`.
  A reference to an alias, qualified or not, lowers to its target, a
  generic instantiation with its arguments substituted (below):

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
  pub fn on_event(name: &str, listener: &Function<fn(JsString) -> Undefined>);
  ```
* A generic alias instantiation `Foo<A, B>` chases to the target with
  the parameters substituted, so `type Info<M> = Request<M> | string`
  used as `send(input: Info<Meta>)` fans out exactly like the written
  union. A bare `Info` substitutes the parameter defaults (`any` when
  none is declared), in order, so a default can name an earlier
  parameter. A qualified instantiation (`Events.Result<number>` for
  `type Result<T> = Pair<T, string>` in `Events`) lowers where it is
  written, so the arguments keep their meaning. Each name in the target,
  or in a default standing in for an omitted argument, that would bind
  differently there is qualified through the path's namespaces:
  `Events.Pair`, or `Events.X` for an `X` that an alias in
  `Events.Inner` takes from `Events` while a global `X` exists. Each
  namespace's export map is read forward, so a name exported under
  several public names is reached by any of them, its own first. A
  target naming something no such path reaches is not chased; the
  reference then names the alias.

  The alias itself is still emitted, as its
  [enum](#named-union-aliases) when it names a union of value arms,
  except a union alias without type
  parameters that erases to `JsValue`: every use lowers through it to its
  members, so `pub type Like = JsValue;` would name nothing. One that
  reaches itself (below) is named by its uses and stays, and so does a
  generic one, which an instantiation that isn't chased names. A Rust alias can't declare a type
  parameter its target doesn't use (E0091), so a parameter the lowered
  target doesn't mention is dropped from the alias. This covers a target
  that never names it as well as one lowering erases; the trailing kept
  parameters keep their lowered defaults, unless one names a dropped
  parameter:

  ```ts
  type Branded<T, B> = T & { __brand: B };
  type Erased<T> = T extends string ? Local : never;
  type Skip<A, B, C> = Pair<A, C>;
  type WithDefault<T, U = string> = Pair<T, U>;
  type Chained<T, U = T> = Pair<U, U>;
  declare function skip(value: Skip<string, number, boolean>): Erased<string>;
  declare function defaults(a: WithDefault<number>, b: Chained<number>): void;
  ```

  ```rust
  pub type Branded<T> = T;
  pub type Erased = JsValue;
  pub type Skip<A, C> = Pair<A, C>;
  pub type WithDefault<T, U = JsString> = Pair<T, U>;
  pub type Chained<U> = Pair<U, U>;
  pub fn skip(value: &Pair<JsString, Boolean>) -> JsValue;
  pub fn defaults(a: &Pair<Number, JsString>, b: &Pair<Number, Number>);
  ```

  An alias's target, defaults included, resolves where the alias is
  declared, through every link of an alias chain, and is qualified from
  where it is used. A namespace that shadows one of its names still gets
  the alias's meaning, and a bundler's re-export of a renamed declaration
  under its original name (`export { ns_Position as Position }` inside
  `ns`) reaches the declaration instead of naming itself again. A union
  alias, bare or qualified (`Ns.Alias`), fans out into overloads only when
  every alternative means the same at the use site; otherwise the
  parameter keeps the alias whole:

  ```ts
  interface Inner { top: string; }
  type TopX = Pair<Inner, Inner>;
  type InnerOrName = Inner | string;
  declare namespace Shadow {
    interface Inner { shadow: boolean; }
    function use(a: TopX, b: InnerOrName): void;
  }
  ```

  ```rust
  pub mod shadow {
      pub type Inner;
      pub fn use_(a: &super::Pair<super::Inner, super::Inner>, b: &JsValue);
  }
  ```
* An alias that reaches itself, through its own target or a parameter
  default, directly or through other aliases, is never chased: expanding
  it would not end. Its references name the alias, generic arguments
  lowered as usual:

  ```ts
  type Next<T> = (next: Next<T>) => void;
  type Ping = (pong: Pong) => void;
  type Pong = (ping: Ping) => void;
  declare function step(next: Next<string>, ping: Ping): void;
  ```

  ```rust
  pub type Next<T> = Function<fn(Next<T>) -> Undefined>;
  pub type Ping = Function<fn(Pong) -> Undefined>;
  pub type Pong = Function<fn(Ping) -> Undefined>;
  pub fn step(next: &Next<JsString>, ping: &Ping);
  ```

  Rust rejects a cyclic `pub type` (E0391), so such an alias only
  generates; the bindings that name it don't compile.
* `export { Local as Public };` (sourceless) → exports what the binding
  `Local` declares in the exporting block under the public name, as an
  entry in its [export map](#the-export-map). A declaration of the block
  named `Public` is a different binding and is not that export. The
  public names are what the module exports, so value bindings import by
  them. The first public name is the declaration's JS name, unless it is
  also exported under its own name; `default` comes last:
  * Functions and variables are emitted once per public name, with no
    alias: `function _connect(...); export { _connect as connect };`
    emits `pub fn connect(...)` bound to the JS export `connect`.
  * A constructor variable on its own is a value nothing refers to by its
    local name, so its class takes the JS name as its Rust name; any
    further names alias it.
  * A class keeps its local name as the Rust type, since the module refers
    to it by that name, but binds `js_name` to the JS name, and its
    constructors and static members carry `js_class` with the same name.
    Each other public name gets an alias (`pub type Server = _Server;`).
    A constructor variable with an instance interface of the same name
    (`interface _Foo {…}; var _Foo: { new(): _Foo }`, or `var _Foo:
    _FooConstructor`) is a class in this sense: the interface's members
    refer to `_Foo`, so the merged class is `_Foo` with `js_name = "Foo"`
    and `pub type Foo = _Foo;`.
  * Types without a runtime binding (interfaces, type aliases, enums) get
    an alias only.
  * A namespace's Rust module is named by its JS name, so a local name
    the module does not export does not appear: `namespace N {…}; export
    { N as M }` emits `pub mod m`, and `function make(); namespace make
    {…}; export { make as create }` emits `create()` and `pub mod create`.
    Its members bind with `js_namespace` set to the JS name. Each other
    public name is a module re-exporting it (`pub mod m2 { pub use
    super::m::*; }`), since `pub use m as m2;` would also import a
    function `m` as `m2`. The names come from the namespace's own
    container, so a module that is only reached, not exported, names it
    the same way. TypeScript keeps a module's exports and locals apart
    but Rust has one namespace, so a namespace the module doesn't export
    by name (a local, or the default export) takes a suffix where an
    exported one has its name: with a local `namespace M` beside
    `export { N as M }`, `N` is `pub mod m` and the local `M` is
    `pub mod m_2`. A type takes a suffix the same way, numbered as a
    type is: a block's local `interface H`, which only a signature
    reaches, beside another block's exported `H` is `H2` with
    `js_name = "H"`.

  A `Local` declared outside the exporting block (an import) is left alone
  and only aliased.
* `export { X as Y } from "...";` (with source) → registered as an import
  from the named module.
* `export default class C`, `export default function f` and `export
  default x` (naming a declaration in the module) bind the declaration as
  the module's `default` export. A default export has no name of its own,
  so the Rust name stays the local one, as for the binding an `export =`
  assigns: the class gets `js_name = "default"` and its constructor and
  statics `js_class = "default"`, and the function or static gets
  `js_name = "default"`. As in TypeScript, `export default x` exports every
  meaning of `x`, so a namespace alone is the default too and its members
  bind through `js_namespace = "default"` in a `pub mod` of its local name.
  `export default x` of a name the module doesn't declare is skipped with
  a diagnostic. Outside a module there is no export to
  bind: `export default class C` and `function f` are emitted as plain
  globals, and `export default x` is skipped.

  ```ts
  declare module "processor" {
    export default class Processor { constructor(name: string); }
  }
  declare module "version" {
    const version: string;
    export default version;
  }
  ```

  ```rust
  pub mod processor {
      #[wasm_bindgen(module = "processor")]
      extern "C" {
          #[wasm_bindgen(extends = Object, js_name = "default")]
          pub type Processor;
          #[wasm_bindgen(constructor, js_class = "default", catch)]
          pub fn new(name: &str) -> Result<Processor, JsValue>;
      }
  }
  pub mod version {
      #[wasm_bindgen(module = "version")]
      extern "C" {
          #[wasm_bindgen(thread_local_v2, js_name = "default")]
          pub static version: String;
      }
  }
  ```

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
concatenation, references to the same enum's members, bare or qualified,
and references to another enum's members through its path
(`Base.Default`, `Ns.Inner.Value`, `Base["Other"]`), including one in
another file. A reference into the member's own declaration can only reach
an earlier member, but one into another declaration of the enum can reach
any. An enum is numeric when any member evaluates to a number, so
`Read = 1 << 0` alone is enough, and a string enum otherwise.

A member without an initialiser is numbered only in a `const enum`, as
TypeScript does: 0 as the first member of its declaration, otherwise one
more than the previous member's value when that is a number, and no value
after a string member or one without a value. Every enum in a `.d.ts` is
ambient, and TypeScript treats a member without an initialiser of an
ambient non-`const` enum as computed rather than auto-incremented: its
value is only known at runtime, so it has no value here either, and is
skipped as below. (tsc's own declaration emit always writes the
initialisers out, `A = 0`.)

```ts
declare enum Computed { A, B }
declare enum Mixed { A = 1, B, C = 4 }
declare const enum Numbered { A, B, C = 10, D }
```

```rust
pub type Computed = Number; // `f64` in signatures
pub enum Mixed { A = 1, C = 4 }
pub enum Numbered { A = 0, B = 1, C = 10, D = 11 }
```

The declarations of a merged enum, those of one name in the same namespace
path of one scope, are evaluated in order as one enum, whose members can
reference each other's, and emitted once at the first. The scope is the
global scope of every file (script files and `declare global` blocks), the
`declare module` blocks of one module, or a module file's top level, so a
same-named enum in an imported module file stays a separate enum:

```ts
declare enum Base { Default = 2 }
declare const enum Mode { A = Base.Default, B, C = A | 8 }
declare enum Merged { A = 1 }
declare enum Merged { B = A + 1 }
```

```rust
pub enum Base { Default = 2 }
pub enum Mode { A = 2, B = 3, C = 10 }
pub enum Merged { A = 1, B = 2 }
```

A member whose value can't be evaluated, such as `B = Undeclared.X`, one
in a reference cycle, a member without an initialiser of a non-`const`
enum, or a `const enum` member continuing from one of these, is skipped
with a diagnostic. Its value is only known at runtime, and a guessed
discriminant could repeat a real member's value and turn that member into
an alias of it. wasm-bindgen can't export an enum without variants, so an
enum left with none is erased with a diagnostic. When no member has an
initialiser it's erased to `number`, like `type Computed = number`, so it
crosses as `f64`: TypeScript requires a computed member to be a number and
accepts any number where the enum is expected. Otherwise it's erased to
`JsValue`, since an initialiser that didn't evaluate, like `Undeclared.X`,
could be a string.

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
they have no `JsGeneric` form, and a string enum has a slice form but no
`Vec` one (numeric enums, which have both, match it so the two kinds
lower alike). An enum follows the primitive it carries wherever that
primitive would not
stay native: inside a `JsGeneric` position (`js_sys` containers,
callbacks, erased local generics) it takes the primitive's wrapper, and
in a slice the primitive itself:

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
exports and lets same-named private copies coexist in one build. A
numeric enum inside a namespace carries the namespace's `js_namespace`,
like its imports, since wasm-bindgen defines a JS object for it. A string
enum is only its values: wasm-bindgen never names it in JS and carries a
`js_namespace` through without reading it, so it has none:

```rust
pub mod timing {
    #[wasm_bindgen(private, js_namespace = "Timing")]
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    #[repr(u32)]
    pub enum Level { /* ... */ }

    #[wasm_bindgen(private)]
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum DurationUnit { /* ... */ }
}
```

## Type-level evaluation

`keyof`, indexed access, mapped types and the object and union utilities
describe a type in terms of others, so they have no lowering of their own.
Once every file is parsed, ts-gen evaluates them over their concrete
operands and rewrites each use in place. The utilities with an evaluation
are `Partial`, `Required`, `Readonly`, `Pick`, `Omit`, `Exclude`, `Extract`
and `NonNullable`; the rest of TypeScript's (`ReturnType`, `Awaited`, …)
approximate to their first argument as before.

```ts
type Unit = "hour" | "minute" | "second";
interface Options { name: string; size?: number; readonly fixed: boolean }

declare function roundTo(unit: Exclude<Unit, "hour">): void;
declare function configure(partial: Partial<Options>, picked: Pick<Options, "name">): void;
type Sized = Pick<Options, "size">;
type OptionKey = keyof Options;
interface Named extends Omit<Options, "size"> { label?: string }
```

emits:

```rust
pub fn round_to(unit: ExcludeUnitHour);
pub fn configure(partial: &PartialOptions, picked: &PickOptionsName);

pub enum ExcludeUnitHour { Minute = "minute", Second = "second" }
pub enum OptionKey { Name = "name", Size = "size", Fixed = "fixed" }

// All-optional dictionaries with builders:
pub type PartialOptions;   // name?, size?, fixed? (readonly)
pub type PickOptionsName;  // name
pub type Sized_;           // size? (`Sized` is in the prelude)

// `Named` has `label`, `name` and `fixed` as its own members and no parent.
```

* **A result named by a reference takes the reference's spelling.** A
  string-literal result named at a use site by a reference, `keyof` or
  indexed access becomes a string enum of that spelling, PascalCased with
  its arguments: `Exclude<Unit, "hour">` is `ExcludeUnitHour`,
  `Plural<"day" | TimeUnit>` is `PluralDayOrTimeUnit`, `keyof Extended` is
  `KeyOfExtended`. An object result becomes a dictionary interface the same
  way (`PartialOptions`). An inline union that evaluates to literals stays
  inline, so a `"auto" | Plural<U>` field still gets its own
  [`<Field>Kind`](#erased-return-position-unions-become-dynamic-union-enums).
* **An argument filling the alias's role replaces it.** A one-argument
  alias name is read as its first PascalCase word, the head, then the
  role its argument plays. When the role's last word is also a word of
  the argument's spelling, the argument names the role, and the result is
  the head followed by the argument: `PartialTemporalLike<ZonedDateTimeLikeObject>`
  is `PartialZonedDateTimeLikeObject` (role `TemporalLike`, argument
  `…LikeObject`), and `PluralizeUnit<"day" | TimeUnit>` is
  `PluralizeDayOrTimeUnit` (role `Unit`). Otherwise, or with a one-word
  alias name (`Plural`) or several arguments, the name is the alias
  followed by its arguments, as is a short name some declaration already
  has.
* **One shape is one type.** Two evaluations with the same members in the
  same scope share a declaration, under the first name given: `Omit<Options,
  "fixed">` and `Pick<Options, "name" | "size">` are the same dictionary. A
  utility that changes nothing keeps the type it was given: `Partial<T>`
  over an all-optional `T` is `T`. So does a mapped type over `keyof T`
  whose result has exactly `T`'s members:
  `PartialTemporalLike<TimeLikeObject>`, which only makes optional what
  already is, is `TimeLikeObject`.
* **An alias to an evaluated type is promoted.** `type Sized = Pick<…>`
  is the dictionary `Sized_`; `type OptionKey = keyof Options` is the enum
  `OptionKey`. A utility over such an alias reads it evaluated, so
  `Exclude<Unit, Minute>` and `Pick<Options, Exclude<OptionKey, "fixed">>`
  work wherever the alias is declared.
* **A dictionary capturing a type parameter is generic over it.** Inside
  `interface Holder<T> { partial: Partial<Box<T>> }` the field is
  `PartialBox<T>`, a dictionary declared with `T` as its own parameter;
  the parameter it stays generic over is left out of its name.
* **A utility in `extends` flattens.** wasm-bindgen inheritance is
  `extends = Parent`, which needs a declared parent, so `Omit<…>`,
  `Pick<…>`, `Partial<…>` and an instantiated generic alias in an
  interface's `extends` list become the interface's own members, after
  the members it declares itself. A property several parents carry
  (`Pick<V, "id">` and `Omit<V, "values">` both carry `id`) is a member
  once, typed by the first parent to name it, and one the interface
  declares itself keeps its own type. A plain parent, generic or not, stays
  `extends = Parent` as before, and `Deref` reaches its members.
* **Generic aliases instantiate only when they must.** `Foo<Bar>` stays a
  Rust generic alias unless `Foo`'s target contains one of these
  constructs, in which case each instantiation evaluates with its
  arguments. The alias's own declaration evaluates at its parameters'
  bounds, so `type Plural<T extends Unit> = T | { … }[T]` is the enum
  `Plural` of every unit, and an instantiation at the bound shares it.
* **A type parameter out of scope evaluates at its bound.** Inside
  `interface Rounding<U extends Unit> { smallestUnit?: Plural<U> }` the
  field is `Plural`, every unit: Rust can't vary an enum per
  instantiation, and the bound is the one type every instantiation is
  assignable to.
* **Mapped types** evaluate over a literal key set. Over `keyof T`
  (possibly through `Exclude`) they are homomorphic: a property keeps the
  optionality and `readonly` it has in `T` unless a `?` / `-?` /
  `readonly` modifier says otherwise, and keeps its type when the mapped
  type gives none. An `as` clause is not followed.
* **Indexed access on an optional property admits `undefined`.**
  `Options["size"]` is `number | undefined`, so a function returning it
  returns `Option<f64>`. `-?` and `Required` remove the `undefined` again
  from a property they make required, as TypeScript does:
  `Required<Options>` has `size: f64`, while a `note: string | undefined`
  that was never optional keeps its `undefined`.
* **`never`** is dropped from a union and erases to `JsValue` alone.
* **What can't be evaluated keeps its old lowering**, with a warning: a
  utility is its first argument (`Partial<Widget>` over a class with
  methods is `Widget`, since a method has no property type), and `keyof`,
  indexed access and mapped types erase to `JsValue`. An unconstrained
  type parameter is the usual cause.

## Multiple-context name resolution

When the same name appears in different `ModuleContext`s (e.g. a global
`interface EmailMessage` and a `cloudflare:email`-scoped class
`EmailMessage`), they remain distinct types. `merge_class_pairs` keys on
`(name, ModuleContext)` to keep them separate. Same-context same-name
still merges as expected, as long as TypeScript allows the pair (see
[Duplicate identifiers](#duplicate-identifiers)).

## Duplicate identifiers

Declarations of one name in one container (a file's top level, a
namespace, a `declare module` body, or the global scope across files)
merge only as TypeScript merges them. TypeScript reports "Duplicate
identifier" for a declaration whose keyword can't merge with one the
name already has:

| Later declaration | Conflicts with an earlier                 |
| ----------------- | ----------------------------------------- |
| `type`            | `type`, `interface`, `class`, `enum`      |
| `class`           | `type`, `class`, `enum`                   |
| `interface`       | `type`, `enum`                            |
| `enum`            | `type`, `interface`, `class`              |

What a `type` alias becomes doesn't matter: one [promoted to an
interface](#type-alias-position), a discriminated union or a string enum
is still an alias, so it merges with no interface. As in TypeScript's
binder, the first declaration keeps the name. The later one is skipped
with a warning pointing at its name, and nothing of it is emitted, not
even the interfaces its inline literals would hoist:

```ts
type Dup = { a: number };
interface Dup { b: string }   // warning: Duplicate identifier `Dup`: interface conflicts with the type alias `Dup` declared earlier, skipping it
```

```rust
pub type Dup;
pub fn a(this: &Dup) -> f64;   // no `b`
```

Everything else still merges: interfaces with interfaces and with a
class, enums with enums, a namespace with a class, function, enum or
alias, an alias with a constructor `var` (see [Constructor variables
named by a type alias](#constructor-variables-named-by-a-type-alias)),
an interface with a constructor `var`, and a function's overloads. Only
these four keywords are checked: a value TypeScript also rejects, such as
a `var` beside a `class` or an `enum`, merges as before.

## External type mapping and web platform defaults

Names that resolve through scope but aren't declared in the input
source — `Blob`, `Headers`, `Event`, `ReadableStream`, `Response`,
`Intl.DateTimeFormatOptions`, … — fall through to the **external map**.
The resolution order at each use site is:

1. **`js_sys::*` glob** for the names listed in `JS_SYS_RESERVED`
   (`Error`, `Promise`, `Map`, `Array`, `Object`, …). Emitted as bare
   idents, no `use` alias needed.
2. **User-supplied `--external` mappings**, in priority order:
   explicit type maps (`Blob=::web_sys::Blob`, or by qualified path,
   `Intl.DateTimeFormatOptions=my_intl::Options`) > module maps
   (`node:buffer=node_buffer_sys`) > wildcard module maps
   (`node:*=node_sys::*`). A qualified path also takes a wildcard
   namespace map (`Intl.*=my_intl::*`, which maps `Intl.Locale` to
   `my_intl::Locale`), below its explicit maps.
3. **Built-in defaults**:
   * The web platform types: `Blob`, `Event`, `Headers`, `DOMException`,
     `ReadableStream`, `Response`, `URL`, `URLSearchParams`, `WebSocket`,
     … all map to their `::web_sys::*` equivalents automatically. The full
     list lives in `external_map::WEB_SYS_DEFAULTS`. Run with
     `--no-web-sys` to disable these defaults (e.g. for environments that
     don't link `web_sys`).
   * The one `js_sys` class whose Rust spelling differs from TS,
     `URIError`, maps to `::js_sys::UriError`
     (`external_map::JS_SYS_RENAMES`).
   * The lib's `Intl` namespace: `Intl.<Name>` maps to
     `::js_sys::Intl::<Name>` for each type js-sys has there
     (`external_map::JS_SYS_INTL`): the `Collator`, `DateTimeFormat`,
     `DisplayNames`, `DurationFormat`, `ListFormat`, `Locale`,
     `NumberFormat`, `PluralRules`, `RelativeTimeFormat` and `Segmenter`
     classes, their `…Options`, `Resolved…Options` and `…Part` types, and
     the few records around them (`SegmentData`, `Segments`, `WeekInfo`,
     `TextInfo`, `Duration`, `LocaleMatcherOptions`). js-sys's string
     enums (`Intl::LocaleMatcher`, …) aren't mapped: TypeScript spells
     those as literal unions, which lower on their own.

   The `js_sys` defaults survive `--no-web-sys`, as they need nothing
   beyond js-sys.
4. **Fallback**: emit a `#[allow(dead_code)] use JsValue as Foo;`
   alias plus an error diagnostic so the output still compiles while
   surfacing the missing mapping. A qualified name erases to `JsValue`
   in place.

User mappings always override the defaults. The `js_sys` short-circuit
is unaffected by `--no-web-sys` — those names are part of the
generated file's `use js_sys::*;` prelude.

A bare name's mapping lands as a `use` alias at the top of the generated
file. A qualified name's maps by its whole path and is spelled in place
(`::js_sys::Intl::Locale`), since its last segment alone may be a local
type's name; so a qualified mapping should be an absolute path
(`::my_intl::Options`, `crate::…`) or start at a crate name. It applies
only where the path's head is the global namespace of that name: a local
namespace named `Intl` shadows the lib's, and a member it lacks stays
unresolved. A global `declare namespace Intl` in an input merges with the
lib's, as in TypeScript: its members are emitted in its `pub mod` and
shadow the lib's of the same name, ahead of any mapping, and the rest
still map.

The lib's two `Intl` aliases that js-sys has no type for, which inputs
use without declaring (`toLocaleString(locales?: Intl.LocalesArgument,
options?: Intl.DateTimeFormatOptions)`), aren't defaults of the map but
built-in aliases, and lower like any other alias. `Intl.UnicodeBCP47LocaleIdentifier` is `string`, and
`Intl.LocalesArgument` is `Intl.UnicodeBCP47LocaleIdentifier | readonly
string[] | undefined`, so a locales parameter fans out into a single
locale and a list of strings, each lowering like any other string
parameter (`&str` and `&[String]`, or `impl JsStringLike` and
`&[impl JsStringLike]` under mono). The
lib's `Intl.Locale` arms are left out: one would add a `_with_locale`
overload to every locales parameter, doubling the overloads with the
trailing options, and in the list it would make the element a union that
erases to `JsValue`. An `Intl::Locale` is passed as its `to_string()`.

An `--external` mapping of either path (or an `Intl.*` wildcard) replaces
the alias, as a mapping of a bare builtin's name does: with
`--external Intl.LocalesArgument=::my_intl::Locales`, a locales parameter
is a single `&::my_intl::Locales`, without the fan-out.

```ts
declare class PlainDate {
  toLocaleString(locales?: Intl.LocalesArgument, options?: Intl.DateTimeFormatOptions): string;
}
```

```rust
pub fn to_locale_string(this: &PlainDate) -> String;
pub fn to_locale_string_with_str(this: &PlainDate, locales: &str) -> String;
pub fn to_locale_string_with_slice(this: &PlainDate, locales: &[String]) -> String;
pub fn to_locale_string_with_str_and_options(
    this: &PlainDate,
    locales: &str,
    options: &::js_sys::Intl::DateTimeFormatOptions,
) -> String;
pub fn to_locale_string_with_slice_and_options(
    this: &PlainDate,
    locales: &[String],
    options: &::js_sys::Intl::DateTimeFormatOptions,
) -> String;
```

Under [per-monomorphization](#in-scope-generic-type-parameters) the single
locale takes `impl JsStringLike` and the list `&[impl JsStringLike]`, as
every `string` and `string[]` argument does.

## Provided functions (`--provides`)

Everything above binds JS *into* Rust. `--provides` is the reverse
direction, wasm-bindgen's export side: what the module provides to JS.
The spec is one of

* a `.d.ts` whose export surface *is* the module's: `export function
  fetch(...)`, `export const api: { math: { add(a: number, b: number):
  number } }` (nested object literals), `export const f: (x: T) => U`
  or a function-type alias (`export const fetch:
  ExportedHandlerFetchHandler<Env>`), and `export default handler` where `declare const handler: T` in the
  same file (`T` an interface, possibly with type arguments, or an
  object literal). Optional members of an interface default are
  selected with a JSDoc `@provides fetch, tail` on the `export default`.
* `Name[.member,...]`: an interface from the inputs that the default
  export implements, with optional members (`fetch?`) listed to
  include them.

Interface members may be methods or properties of function type,
through type aliases including generic instantiations
(`fetch?: ExportedHandlerFetchHandler<Env, Cf>` in workers-types). The
interface's type parameters take the given arguments, else their
declared defaults, else `unknown`. Members inherited through `extends`
count as the interface's own, each clause's type arguments taken under
that substitution (`interface MyHandler<E> extends ExportedHandler<E>`);
a member declared nearer hides an inherited one of the same name. Required members are always
provided; an unselected optional member is skipped with a warning
naming it, and a selected member that is not a function is an error.
Generic functions and non-function properties warn and skip;
overloads, variadic parameters and other export forms are errors. A
spec that resolves to nothing is an error.

Each provided function sits at a JS path (`default.fetch`, `fetch`,
`api.math.add`). Its Rust name is the path in snake case
(`api_math_add`); a `default.` member drops the prefix (`fetch`) unless
another provider already has that name, in which case it keeps it
(`default_add` beside a top-level `add`). Any remaining clash
(`foo.bar` beside `foo_bar`) is an error naming both paths. Parameters
are received **by value** (`Request`, not `&Request`; `JsValue` for
`any`), an optional parameter as `Option<T>`, and results are owned.
A nullable parameter is `Option<T>` too: in this direction the glue
reads both `null` and `undefined` as `None`, unlike import arguments.
A result is a write, so it keeps the exact carrier as an import's
argument does: `string | null` returns `Nullable<String>`, and JS
receives `null`, where an `Option<String>` would hand it `undefined`.
A `Promise<T>` or `T | Promise<T>` result is `async fn -> Result<T,
JsValue>`. The wrapper and scaffold import `js_sys::*` alongside the
bindings, so `Record<string, number>` (`Object<Number>`), callbacks
(`Function<fn(..)>`), `symbol`, `bigint` and object literals lower as
in the bindings.

Two files are written next to the output:

* `provides.rs` (regenerated): one `#[wasm_bindgen]` function per
  provided function, delegating to the app module (`--provides-app`,
  default `super::app`) and importing types from the bindings module
  (`--provides-bindings`, default `super::<output stem>`, with `-` and
  `.` replaced by `_`).

  ```rust
  #[wasm_bindgen(js_namespace = default, js_name = "fetch")]
  pub async fn fetch(request: Request, env: Env, ctx: ExecutionContext)
      -> Result<Response, JsValue> {
      super::app::fetch(request, env, ctx).await
  }
  ```

  wasm-bindgen's `js_namespace` on an exported function spells the
  path: `default` produces `export default { fetch }`, `["api",
  "math"]` the nested object. The integration tests run wasm-bindgen
  over the built crate and check the resulting module in node:
  `m.version()`, `m.api.math.add`, `m.default.issue`. wasm-bindgen's
  `web` target reserves the default export for its init function.
* the app scaffold, named after the last segment of `--provides-app`
  (`app.rs`), only if absent: the same signatures with the TS JSDoc as
  doc comments and placeholder bodies, for the author to fill in.

## Wasm ABI (`--target wat` / `embed`)

Besides the default `--target rust` output (wasm-bindgen extern
blocks), ts-gen can emit a lower-level **Wasm ABI** suitable for
languages that compile directly to WebAssembly without wasm-bindgen.
`--target wat` prints it as an import template: every ABI function as
an import from `./bindings.wasm`, with its TS declaration alongside,
and a stub export per provided function. `ts-gen embed` implements
the ABI for a module written against it.

The ABI covers what the Rust bindings emit: each
[export](#the-export-map) of the files and modules `--export` selects,
less externalised modules, and the types their signatures reach, a
class reached that way with only its instance members (see
[Global scope](#global-scope)).

### Scalar mapping

| TypeScript         | Wasm type    | Notes                                |
| ------------------ | ------------ | ------------------------------------ |
| `number`           | `f64`        |                                      |
| `boolean`          | `i32`        | `0` = false, non-zero = true; `embed` hands them to JS through `Boolean()` so JS sees `true`/`false`, not `1`/`0` |
| `bigint`           | `i64`        |                                      |
| everything else    | `externref`  | strings, objects, null, undefined, numeric enums, `never` |

A reference to a type alias reads as its target, through aliases of
aliases and generic instantiations, as the Rust bindings lower it:
`type Millis = number` and lib.dom's `DOMHighResTimeStamp` are `f64`,
and a `type Nothing = void` return has no result. A type parameter
stays `externref`.

Nullable scalars (`number | null`) cross as a nullable reference;
there is no sentinel for "null" in `i32`/`f64`/`i64`. `Number.value`
unboxes one that is present, and reads a numeric enum member. A
`void`/`undefined` return has no result slot, while `null` and `never`
returns are `(result externref)`.

### Nullability and the sys surface

The wire type is `externref` = wasm-bindgen's `JsValue`: values cross
exactly as JS produced them, with no `?? null` normalisation, and
every consumer reproduces wasm-bindgen's `sys` surface over it
(`JsValue`, `JsNullable<T>` = `T | null | undefined`, `JsOption<T>` =
`T | undefined`, `Null`, `Undefined`). The ABI records for each
reference position whether its TS type admits `null` and whether it
admits `undefined`; the template spells both `externref`. Only `null`
is a wasm null, so a consumer may take a position that does not admit
`null` as `(ref extern)`. The bits are computed through type aliases
(including generic instantiations, with the arguments substituted),
qualified paths (`NS.Inner`) and union arms; a type parameter may be
instantiated with anything and so admits both, as does a name that
does not resolve. Discriminants are read from the value, never
encoded in the signature, and errors cross as `externref`, one type.

Two ABI functions, one global and one JSPI function exist for every
`.d.ts`:

| Import                                    | Meaning                          |
| ----------------------------------------- | -------------------------------- |
| `"./bindings.wasm" "JsValue.is_undefined"`| `x === undefined` → `i32`        |
| `"./bindings.wasm" "Number.value"`        | `Number(x)` → `f64`              |
| `"./builtins.js" "undefined"`             | immutable `externref` global     |
| `"./builtins.js" "await"`                 | JSPI suspend: promise → settled value |

`is_null` needs nothing: `ref.is_null` in the language. `Number.value`
is JS coercion, not a type test: `Number(null)` is `0`,
`Number(undefined)`, non-numeric strings and plain objects are `NaN`,
`Number("5")` is `5`, and `Number(Symbol())` throws; test
`ref.is_null` / `is_undefined` first when the distinction matters.

`await` is `(func (param externref) (result externref))`, JSPI's
`WebAssembly.Suspending`: it suspends the module on a promise and
returns the settled value. It is not part of the `.d.ts`'s ABI and
the template imports it so a module can use it; importing it is
optional and opts the module into JSPI. The wrapper that publishes
the provided functions then wraps each with `WebAssembly.promising`,
so a handler may suspend and JS receives a promise of its result.
Without the import a handler returns a promise (or a plain value)
itself, and the published functions are the raw exports.

### Reference types

The template uses `externref` for all JS values. `embed` validates
each reference position (each param, the result) independently and
accepts, per position:

* `externref` or `anyref` (GC languages emit `anyref` for `@external`
  parameters and `externref` for results; any mix is fine);
* `(ref extern)` or `(ref any)` wherever it is sound. Positions the
  module itself produces (its import arguments, its handler results)
  may always be narrowed: that is the module's own promise. Positions
  JS produces (import results, handler parameters, the `await` result)
  may be narrowed unless the TS type admits `null`; `undefined` is a
  non-null reference, so `T | undefined` qualifies. The bindings export
  the exact type and wrap a narrowed result in `ref.as_non_null`, so a
  JS `null` where TS promised a value traps rather than crossing. A
  non-null reference on a `null`-admitting JS-produced position is
  rejected with an error naming it.

The generated bindings mirror the module's exact types.

### Export naming

Each callable gets an import name of the form `Type.member`, where
`Type` is the owner's dotted JS path (`WebAssembly.Module.exports`
for a class inside a namespace):

```wat
(import "./bindings.wasm" "Response.status" (func ...))
(import "./bindings.wasm" "Response.clone" (func ...))
```

Overloads of one member expand together through the Rust bindings'
signature flattening (optional truncation, union arms, `_with_<x>`
suffixes), run without the Rust type context. A union behind a type
alias is not resolved there and stays one parameter. Expansions whose
parameters render alike merge into one ABI function, and without the
context every reference outside `js_sys` renders as `JsValue`. Such a
function takes one `externref` and makes the same JS call whichever arm
the caller passes. Suffixes are computed from the expansions that
remain, so ABI names match the Rust bindings only where both keep the
same variants. `new(body?: BodyInit | null, init?: ResponseInit)` is:

```wat
(import "./bindings.wasm" "Response.new" (func ...))
(import "./bindings.wasm" "Response.new_with_body" (func ...))
(import "./bindings.wasm" "Response.new_with_body_and_init" (func ...))
```

named from the parameter, where the Rust bindings have one constructor
per `BodyInit` arm (`new_with_str`, `new_with_array_buffer`,
`new_with_blob`, …). Inline arms that still render differently stay
apart in both: `(x: string | number)` is `m` and `m_with_f64`, while
`(x: Blob | FormData)` is one ABI `n` against Rust's `n` and
`n_with_form_data`.

A type or namespace exported under several names has each member under
each name, as one function `bindings.wasm` exports under all of them.
`Type` is first the name it binds by, the class's `js_name`, the
interface's local name or the namespace's `js_namespace`, then each
other public name (`export { _N as N1, _N as N2 }` gives `N1.f` and
`N2.f`):

```ts
declare module "db" {
  class _Conn { constructor(); get(): string; }
  export { _Conn as Conn, _Conn as Connection };
}
```

```wat
(import "db" "Conn.new" (func $db:Conn.new (result externref)))
(import "db" "Connection.new" (func $db:Connection.new (result externref)))
(import "db" "Conn.get" (func $db:Conn.get (param $this externref) (result externref)))
(import "db" "Connection.get" (func $db:Connection.get (param $this externref) (result externref)))
```

Parameters, a provided function's included, are named as in a Rust
parameter list: sanitised, and numbered where they collide with each
other or with the receiver `this`, so `collideArgs(a, $a, fooBar, foo_bar)`
has parameters `a`, `a2`, `foo_bar` and `foo_bar2`, and `method(_this)`
has `this` and `this2`. A non-ASCII character becomes `_`, since WAT
identifiers are ASCII (`café` → `caf_`).

### Member types

| TypeScript member        | ABI function shape                           |
| ------------------------ | -------------------------------------------- |
| Instance method `m()`    | `(param $this externref) ...args...`         |
| Instance getter `x`      | `(param $this externref) (result T)`         |
| Instance setter `x=`     | `(param $this externref) (param $value T)`   |
| Static method `M.m()`    | `...args...`                                 |
| Static getter `M.x`      | `(result T)`                                 |
| Static setter `M.x=`     | `(param $value T)`                           |
| Constructor              | `...args... (result externref)`              |
| Dictionary `new`         | `(result externref)` — creates `{}`          |
| `declare var x: T`       | `(result T)` — reads `globalThis.x`          |
| `declare const x: T`     | `(result T)` — reads `globalThis.x`          |

`T` is the wire type of the TS type: `declare let n: number` is
`(result f64)`.

### Dictionaries

Dictionaries (property-only interfaces) get a `Type.new` function
that returns an empty JS object (`{}`). Callers then use the
generated setters to populate fields:

```wat
(call $ResponseInit.new)
(local.set $init)
(call $ResponseInit.set_status (local.get $init) (f64.const 200))
```

### Global variables

`declare var x: T` and `declare const x: T` generate a getter that
reads the property from `globalThis`:

```wat
(import "./bindings.wasm" "console" (func $console (result externref)))
```

The returned object can then be used with instance methods from its
interface type (e.g. `Console.log`).

### Namespaces

Namespace members use the dotted path as a prefix:

```wat
(import "./bindings.wasm" "WebAssembly.compile" (func ...))
```

Free functions (global scope) use just the function name:

```wat
(import "./bindings.wasm" "fetch" (func ...))
```

### Module-scoped declarations

Declarations inside `declare module "<spec>" { ... }` keep the same
`Type.member` names, and the template imports them from `"<spec>"`
instead of `./bindings.wasm`. The import module and name together are
unique, so two modules exporting the same name are two imports:

```wat
(import "node:fs" "constants.O_RDONLY" (func $node:fs:constants.O_RDONLY (result f64)))
(import "node:os" "constants.O_RDONLY" (func $node:os:constants.O_RDONLY (result f64)))
```

Every value is reached through one of the module's exports, as
`(<spec>, path)`: `path[0]` is the export, each further segment a
property read, so `constants.O_RDONLY` reads `O_RDONLY` from the
`constants` export of `"node:fs"`. A function, variable or namespace
member is reached this way; a class's constructor and statics go
through the class's export, and its instance members through the
receiver as for a global class. Types (interfaces, aliases) are not
values and import nothing. A scalar `const` or `let` that is itself an
export takes its scalar type, as any other value does: `export const
count: number` is `(func (result f64))`.

`embed` links the language module's imports from `"<spec>"` to
bindings functions, exported as `<spec>:<name>` (under the bindings'
private prefix, see [Single-module output](#single-module-output)),
before the merge:
`wasm-merge` links by module name, and the bindings themselves import
the real `"<spec>"`. They reach each value through the merged module's
own ESM imports, one immutable global per export, an `externref`
unless the export is a scalar (ESM integration coerces a
non-`WebAssembly.Global` export into a global of the import's type), with
each further path segment a `reflect_get` keyed by a
`wasm:js/string-constants` import:

```wat
(import "cloudflare:sockets" "connect" (global $m:cloudflare:sockets:connect externref))
(import "cloudflare:sockets" "Socket" (global $m:cloudflare:sockets:Socket externref))
```

A static import is a snapshot of the export at instantiation, not a
live binding: a module that reassigns an exported `let` later is not
seen. A module's free function is called with `this` undefined, as an
imported function is. A class export is constructed with `call_new`,
and its statics and nested namespaces are read with `reflect_get` as
for globals. A `const`/`let` export is the global's value, under the
per-position reference rule above (`anyref` converts, `(ref extern)`
narrows). A scalar one is a global of its type, `(global f64)` for a
`number` and `(global i64)` for a `bigint`. The JS API rejects a
boolean for an `i32` global, so a `boolean` stays an `externref`
global, read as `i32` through `Boolean` imported from `builtins.js` as
`(func (param externref) (result i32))`:

```wat
(import "embed:values" "count" (global $m:embed:values:count_f f64))
(import "embed:values" "enabled" (global $m:embed:values:enabled externref))
```

Assigning a module export is rejected by `embed`. A language module
may still import other exports of `"<spec>"` directly, as anything but
a function; those are left for the host to resolve.

### Provided functions in the ABI

The [`--provides`](#provided-functions---provides) spec is lowered to
handlers: one function per provided function, parameters by value,
result the owned type. A `Promise<T>` result is `T`'s wire type, or a
nullable `externref` for `Promise<void>`: the slot carries the promise
itself, or the settled value where a JSPI wrapper supplies the
promise. Only a plain `void` export has no result. The wasm export
name is the JS path joined with `.` with a leading `default.` dropped
unless a top-level provider has the same path: `fetch`,
`api.math.add`, `default.add` beside `add`.

The template emits a stub per provided function, annotated with its JS
path:

```wat
;; default.fetch: fetch(request: Request, env: E, ctx: ExecutionContext): Promise<Response>
(func (export "fetch") (param $request externref) (param $env externref) (param $ctx externref) (result externref)
  unreachable)
```

`embed` publishes the module namespace: each distinct first path
segment becomes an exported externref global holding the function
itself (`version`) or an object tree (`default.fetch`,
`api.math.add`), built with `reflect_set` at start. A handler with a
`boolean` result (an `i32`) is published through a trampoline that
passes it to `Boolean()`, so JS receives `true`/`false` as from
the bindings; every other result reaches JS as the module returns
it. The module's raw function exports are stripped after the merge; any other module
export named like a published segment (a raw `version` beside
`version.get`, a raw `default`) would survive it as a duplicate and
is an error. Without `--provides` the wrapper is empty and the
module's own exports pass through.

### JSPI detection

Importing `await` from `builtins.js` opts into JSPI (JavaScript
Promise Integration) mode:

```wat
(import "./builtins.js" "await" (func $await (param externref) (result externref)))
```

Each position is independently `externref` or `anyref`; the result
stays nullable since a promise may resolve to `null`. In JSPI mode,
`embed` wraps each provided function with `WebAssembly.promising` so
it can suspend on Promise results; since only provided functions are
wrapped, `await` without `--provides` is an error. Without the
import, handlers must return Promises directly (or non-Promise
values).

### The three artifacts

`ts-gen embed` reads a language module written against the template
(given the same `--export`s and `--external`s, so it binds the same
surface) and produces:

1. **`bindings.wasm`** — implements each `Type.member` import using
   the JS primitives in `builtins.js`. Imports of one function under
   several of its names with the same signature share one bindings
   function, exported under each (`(func (export
   "ts-gen:bindings.Conn.get") (export "ts-gen:bindings.Connection.get")
   ...)`).
2. **`wrapper.wasm`** — imports the language module's provided
   functions and publishes the module namespace: one exported
   externref global per first path segment (`default`, `version`,
   `api`), each wrapped with `WebAssembly.promising` in JSPI mode.
3. **`builtins.js`** — the fixed primitive set both modules import:
   `global` (globalThis), `reflect_get`, `reflect_set`,
   `reflect_apply`, `call0..8`, `call_new0..8`, `is_undefined`,
   `boolean` (`Boolean`, imported as a function so a boolean
   conversion needs no start function), `undefined`, and `await`. Beyond eight arguments, and for
   variadics, the arguments are gathered into a JS array for
   `Reflect.apply` / `Reflect.construct`.

### Single-module output

`embed` links the three modules into a single `.wasm` that imports
only `builtins.js`, the js-string-builtins string constants module
and each `declare module` specifier the module uses. Binaryen's
`wasm-merge` is required for this step: `--wasm-merge`, then
`$WASM_MERGE`, then `PATH`.

Every bindings export carries a private prefix (`ts-gen:bindings.`),
and `embed` points every module import from `./bindings.wasm` at it,
as the wrapper's globals carry `ts-gen:export.`. No export of the
three can clash, so the global `fetch` the module imports and the
`fetch` handler it exports are both linked; a clash `wasm-merge`
still found would be an error rather than a dropped export. The
bindings have no start function, so a language module's own start
function may call them whatever order `wasm-merge` runs start
functions in.

After merging, the bindings' functions and the module's raw export
functions are stripped, leaving the published namespace globals plus
any other exports the module had. An import from `./bindings.wasm`,
the wrapper or the module's specifier left in the merged module is an
error.

The result is loaded as an ES module: compiled with
`importedStringConstants: "wasm:js/string-constants"` and the
`js-string` builtins, with `./builtins.js` and the `declare module`
specifiers resolved by the host. The published exports are
`WebAssembly.Global`s; ESM integration (the `wasm_esm_integration_builtins`
runtime in workerd) presents their values as the module's exports,
so `export default` is the handler object. Instantiated by hand, read
`exports.default.value`.

### Limitations

**Variadics** (`...rest: T[]`) become a Wasm GC array parameter
`(ref null $anyrefs)` where `$anyrefs` is `(type $anyrefs (array (mut
anyref)))`. The `anyref` element type lets GC languages build the
array natively. Fixed parameters before the rest are allowed; the
bindings copy the GC array into a JS array and call `Reflect.apply`.
This requires Wasm GC, not just reference-types.

**Duplicate names.** Within one module, or among the globals, a
declaration merged from several `.d.ts` statements contributes each
member once. The same name in two modules, or globally and in a
module, is two imports. `workers-types` declares `interface
EmailMessage` globally, a dictionary, and its constructor in
`"cloudflare:email"`, so `"./bindings.wasm" "EmailMessage.new"`
creates `{}` and `"cloudflare:email" "EmailMessage.new"` constructs
the class.
