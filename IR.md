# The ts-gen IR and the `tw` bridge ABI

The IR describes a JS API as a set of imports with Wasm-level types, so that
a language can bind it without knowing TypeScript. A separate pipeline, the
**bridge**, implements those imports on top of JS. This document specifies
the IR text format, the wire ABI between a language module and the bridge,
and the behaviour the bridge guarantees towards JS.

Deferred features are listed in [Future work](#22-future-work), and open
questions in [Open items](#open-items).

## Contents

1. [Framing](#1-framing)
2. [Lexical structure](#2-lexical-structure)
3. [Types](#3-types)
4. [Names and containers](#4-names-and-containers)
5. [Modules and import modules](#5-modules-and-import-modules)
6. [Members and keys](#6-members-and-keys)
7. [Overloads and the `@js` call DSL](#7-overloads-and-the-js-call-dsl)
8. [Enums](#8-enums)
9. [Nullability](#9-nullability)
10. [Primitives and validation](#10-primitives-and-validation)
11. [Sequences and memory](#11-sequences-and-memory)
12. [Profiles](#12-profiles)
13. [Boxes, reference wire types and generics](#13-boxes-reference-wire-types-and-generics)
14. [Handles](#14-handles)
15. [Exceptions](#15-exceptions)
16. [Async](#16-async)
17. [Exports](#17-exports)
18. [Callbacks](#18-callbacks)
19. [The `tw` protocol](#19-the-tw-protocol)
20. [End-to-end example](#20-end-to-end-example)
21. [Grammar](#21-grammar)
22. [Future work](#22-future-work)
23. [Open items](#open-items)

## 1. Framing

### Two ABIs

* The **bridge ABI** sits between the language module and the bridge (Wasm
  to Wasm). It's made of Wasm values: scalars, `(ptr, len)` descriptors, GC
  arrays and structs, `externref` and `anyref`. The IR's pure layer
  describes it.
* The **JS ABI** sits between the bridge and JS. It's defined by each type's
  JS representation plus `@js(...)` attributes.

The bridge converts between the two, validating everything that comes from
JS, since that side is untrusted.

**Invariant:** remove every `@js(...)` and what's left is exactly the bridge
ABI. `@js` never changes an import name or a wire type. Semantic attributes
(`@fallback`, `@once`) stay, because they change bridge ABI behaviour.
Generators may read `@js` as facts about JS (Kotlin properties, Rust
nullable carriers), but they never need it to produce a correct call.

The bridge ABI is meant to stand on its own: a host without JS could
implement the same imports.

### Two tag levels

* **TypeScript → IR:** tags in `.d.ts` comments, or producer configuration,
  steer the producer. For example, `@tw-abi u16` on a `number`, `nullish`,
  `@throws`. They never appear in the IR.
* **The IR** has types and `@js(...)` only. There are no ABI attributes:
  `u16` is just a type.

### Producers, bridges and generators

* **The producer** (ts-gen) writes one canonical IR per `.d.ts`.
* **Language generators** read the IR and write bindings in their own naming
  (recasing, escaping, presets).
* **The bridge generator** reads the IR, a [profile](#12-profiles), and the
  compiled language module's imports and exports. It writes the bridge: a
  Wasm module, plus ESM JS glue for the JS ABI.

Example. The IR

```ts
ir 1;
class Request {
  get url(): str;
}
```

makes the language import `("tw:js/global", "Request#get:url()")`:

* linear profile: `(param externref) (result i32 i32)`;
* GC profile, `gc` string carrier: `(param externref) (result (ref $str))`.

In JS the bridge reads `request.url`, checks `typeof === "string"`, and
copies the string into the language's carrier.

## 2. Lexical structure

* The file is UTF-8 and starts with the header `ir 1;`. A reader rejects a
  missing header or any other version.
* `//` starts a comment that runs to the end of the line. `///` starts a doc
  line (`////` is an ordinary comment). Consecutive doc lines, each with
  `///` and one following space removed, are joined with `\n` and become the
  doc of the next declaration, member or enum arm.
* An identifier is a letter, `_` or `$`, followed by letters, digits, `_` or
  `$` (Unicode letters and digits included). A name that isn't an identifier
  is written as a string: `get "content-type"(): str;`.
* Strings are `"..."`, with the escapes `\"`, `\\`, `\n`, `\r`, `\t` and
  `\u{h+}`.
* Integer literals are decimal, optionally negative.
* Keywords are contextual: `ir lib with namespace class interface enum
  extends static get set new export module throws mut fn as in`. A keyword is still a name
  where a name is expected: `get(name: str)` is a method named `get`.
* **Built-in type names** are reserved: `unit`, `bool`, `i8`, `u8`, `i16`,
  `u16`, `i32`, `u32`, `i53`, `u53`, `i64`, `u64`, `f16`, `f32`, `f64`,
  `bigint`, `str`, `arr`, `future`, `fn`, `Any`, `err`, the boxes
  (`Bool`, `I8` … `F64`, `BigInt`, `Str`, `Unit`, `Vec`, `Arr`, `Tuple`).
  * A user declaration with one of these names is written under an escaped
    name, with `@js(name = "...")` carrying the JS name: `@js(name = "List")
    class List_ {}`. The producer escapes keywords the same way.
* Attributes go before a declaration, member or arm, on the line before or
  inline (`@js("GET") Get;`). Several `@js(...)` on one node are one entry
  list:
  * `@js(...)` (see [§7](#7-overloads-and-the-js-call-dsl));
  * `@fallback` ([§8](#8-enums));
  * `@once` ([§14](#14-handles)).

## 3. Types

### Built-in types

Lowercase types are **unboxed**: the bridge converts them into the
language's own representation. The matching uppercase type is the **box**:
an opaque value the bridge hasn't converted
([§13](#13-boxes-reference-wire-types-and-generics)).

| type | box | JS value | notes |
|---|---|---|---|
| `unit` | `Unit` | `undefined` | no wire values; a result's "no result" |
| `never` | — | — | results only: the call never returns normally, or the future never resolves |
| `bool` | `Bool` | boolean | |
| `i8` `u8` `i16` `u16` `i32` `u32` | `I8` … `U32` | number | |
| `i53` `u53` | `I53` `U53` | number (safe integer) | |
| `i64` `u64` | `I64` `U64` | bigint (number also accepted incoming) | |
| `bigint` | `BigInt` | bigint | arbitrary precision ([§11](#11-sequences-and-memory)); std's `BigInt` is its box |
| `f16` `f32` `f64` | `F16` `F32` `F64` | number | |
| `str` | `Str` | string | |
| `T[]` | `Vec<T>` | typed array | `T` is a typed-array element type |
| `arr<T>` | `Arr<T>` | Array | any element type |
| `(A, B, …)` | `Tuple<A, B, …>` | fixed-length Array | `(A,)` is a one-tuple; `()` is invalid |
| `enum` path | itself, as a type argument | per arm | the box is the arm's host value ([§13](#13-boxes-reference-wire-types-and-generics)) |
| class or interface path | itself | the object | always a reference |
| `Any` | — | any JS value except null and undefined | |
| std `Symbol` | itself | symbol | a class the bridge tests with `typeof === "symbol"`, not `instanceof` |
| `future<T>`, `future<T, E>` | — | Promise | [§16](#16-async) |
| `fn(A, …) -> R throws E`, `fn(A, ...arr<T>)` | — | function | [§18](#18-callbacks) |
| `*T[]`, `*mut T[]` | — | typed array over the memory | zero-copy view, import parameters only ([§11](#11-sequences-and-memory)) |
| `mut T[]` | — | typed array | writeback, parameters only |
| type parameter `T` | — | per instantiation | [§13](#13-boxes-reference-wire-types-and-generics) |
| associated type | — | per instantiation | [§13](#13-boxes-reference-wire-types-and-generics) |

* `T?` is the nullable form of any type except `unit` and `never`, and
  `T??` its nested optional ([§9](#9-nullability)): `undefined`, `null` or
  a `T`. `?` nests once, so `T???` is invalid.
* `unit` is allowed in any position (`Pair<unit, Str>`, `x: unit`), except
  as an array element (`arr<unit>` is invalid). It has no wire values. JS →
  Wasm a `unit` value is ignored, not checked, since a TS `() => void`
  callback may return anything. Wasm → JS it's `undefined`. Its box `Unit`
  is always `undefined`, so boxing gives `undefined` and unboxing ignores
  the value.
* `never` is a member's or a function type's result, or a `future`'s `T`,
  and a validation error anywhere else. Its wire has no results. If the JS
  call returns anyway, the bridge throws a `TypeError`, which a `throws`
  member reads as its enum's arm (§15). If a `future<never>` resolves, the
  bridge rejects it with a `TypeError` instead. Generators map it to their bottom type (Rust `!`, Kotlin
  `Nothing`), or to `unit` with the bridge's check standing in for it.
* Box names avoid the JS lib classes, so the lib's `String`, `Array`,
  `Number` and `Boolean` stay ordinary classes, with identity and methods.

### Postfix operators and precedence

* `?` and `[]` are postfix, and apply left to right:
  * `u8[]?` is a nullable typed array;
  * `str??` is `(str?)?`;
  * `arr<str?>` is an array of nullable strings, and `arr<str>?` a
    nullable array.
* A function type's result extends as far right as it can, so `fn(Event) ->
  str?` returns `str?`. A nullable function type with a result needs
  parentheses: `(fn(Event) -> str)?`. `fn(Event)?` (with no arrow) is a
  nullable function.
* `(T)` groups. `(T,)` is a one-tuple.
* `@js(null)` is a prefix attribute on a `T?` at any depth, and takes the
  whole type after it, postfix included: `arr<@js(null) f64?>`,
  `fn(@js(null) Str?)`, `@js(null) u8[]?` ([§9](#9-nullability)). As the
  operand of a postfix operator it's parenthesized, `(@js(null) T?)[]`.
* `*`, `*mut` and `mut` are prefixes that apply only to a whole `T[]`, and
  to nothing else: `*u8[]` is a view, not an array of pointers, and `*u8`
  is a syntax error.

* `?` is invalid on `*T[]` and `*mut T[]`. `mut T[]?` is allowed and uses
  the `T[]?` encoding ([§11](#11-sequences-and-memory)); an absent value
  has no writeback.

### Canonical spelling

Keys (§6) and protocol names (§19) print types in canonical form:

* no spaces, except one space after `mut` and one space on each side of
  `throws`;
* paths are dotted;
* type arguments are comma-separated;
* `->` is written without spaces;
* a type declared inside a `module` block is always qualified by its quoted
  module spec, `"spec".Name`. Global types stay bare. So
  `Buffer#concat("node:buffer".Buffer)` and `Buffer#concat(Buffer)` never
  collide;
* a type parameter or associated type keeps its name;
* `@js(null)` isn't part of it: `arr<@js(null) f64?>` is `arr<f64?>`.

* the built-in error enum is explicit after `throws` (`throws err`), and
  implicit in `future` (`future<T, err>` is `future<T>`);
* a rest parameter is written `...arr<T>`, with no space after `...`
  (`fn(str,...arr<f64>)`);
* a function type omits a `unit` result unless it has `throws`
  (`fn(Event)`, `fn(Event)->unit throws err`);
* a function-typed operand of `?` or `[]` whose tail would absorb the
  operator is parenthesized (`(fn(Event)->str)?`), as is the element of
  `*`, `*mut` or `mut` that isn't a primary type (`*(u8?)[]`).

Examples: `Map<Str,f64>`, `arr<(u32,str)>`, `fn(f64,u32)->Str`,
`*mut u8[]`, `mut u8[]`, `fn(Event)->unit throws KvError`.

### Type aliases

`type Name<T, U = D> = type;` names a type, in any container (§4). A
reference gives the alias's arguments, or leaves trailing ones with
defaults out (`Next<f64>`), and resolves as any type path does.

```ts
class Event {}
type Handler = fn(Event);
type Next<T> = fn(Next<T>, T) -> T;
on(h: Handler);
next(n: Next<f64>);
```

Aliases are transparent: the canonical spelling of a reference is the
spelling of the alias's type with the arguments in place, so renaming an
alias never changes a key or a protocol name. The one exception is a
**back-edge**, a reference to an alias reached while spelling that same
alias's expansion. It spells as its path and canonical arguments, and its
expansion stops there:

* `on(h: Handler)` has the key `on(fn(Event))`;
* `next(n: Next<f64>)` has the key `next(fn(Next<f64>,f64)->f64)`, and the
  trampoline for `Next<f64>` is `tw:call:fn(Next<f64>,f64)->f64`.

Which references are back-edges depends on where the spelling starts, so
a type is spelled from its reference, never from an expansion. In
`fn(Next<f64>,f64)->f64` above, the parameter `Next<f64>` spelled on its
own is `fn(Next<f64>,f64)->f64` again. The bridge and the generators name
a nested function value by its own spelling that way, so a recursive
callback has one trampoline.

An alias's type is valid in any position: not `never`, a view or a
`mut T[]`. Recursion passes through a type constructor, and every
recursive alias is a function type, since a back-edge lays out only as a
function value (§10). These are validation errors:

* `type A = A`, `type A = A?` and `type A = B; type B = A`, with no
  constructor between;
* `type Tree = arr<Tree>`, `type A = fn(A)?` and, in
  `type A = fn(B); type B = arr<A>`, `B`: recursive but not a function
  type;
* `Next<T>` outside an alias, with `T` a type parameter of the member or
  class: its back-edge has no canonical spelling to name a trampoline by.

## 4. Names and containers

* The IR name is the JS name, verbatim. Names the producer synthesizes are
  camelCase.
* `@js(name = "...")` renames a single segment: the IR name is the escaped
  or synthesized name, and the JS name is the string. Dots are never allowed.
  Paths come from nesting.

### Namespaces

`namespace a { namespace b { ... } }` forms the JS path `a.b`. A namespace
may merge with a function of the same name:

```ts
foo(): unit;
namespace foo {
  baz(): unit;
}
```

gives the imports `foo()` and `foo.baz()`. A class or enum merged with a
namespace folds the namespace's members into its statics.

### Paths

A type path resolves lexically, where it's written:

* A one-segment path names a type parameter or associated type in scope
  first.
* Otherwise each enclosing container is tried from the innermost outwards:
  the namespaces, then the top level, then, inside a `module` block, the
  globals. `A.B` names the declaration whose full path is the container's
  path followed by `A.B`, at the first container that has one. So a nearer
  `A` shadows an outer one.
* `"spec".A.B` names `A.B` at the top level of that `module` block, from
  anywhere. The spec is always quoted: an unquoted `spec.A.B` is a
  lexical path through a namespace `spec`.
* `.A.B` is rooted: it names the global `A.B`, past any nearer declaration,
  inside a `module` block too.
* A lexical path of more than one segment can't start with a built-in type
  name (`str.X` would read as `str`); it's a validation error, written
  rooted (`.str.X`) or module-based (`"spec".str.X`) instead.

The producer writes a path that resolves at the use to the declaration the
source named: the plain name when nothing shadows it, otherwise the
shortest qualified path that resolves to it, otherwise the path based at
its `module` block or rooted.

### Classes and interfaces

* `class` is **nominal**. Every class gets the bridge-derived check
  `("tw:instanceof", "<path>")`, of type `(externref) -> i32`, which runs
  `value instanceof <path>`. Downcasting is that check followed by an
  unchecked cast in the language.
* `interface` is **structural**. It never gets a check, and casts to it are
  free and unchecked.
* `extends A, B` is an ordered list, and the first parent is primary (Rust
  generators use it for `Deref`).
  * Upcasting along `extends` is free.
  * A class may extend interfaces, and an interface may extend classes.
* `:` is only ever a type annotation or a carrier. It never means
  inheritance.

```ts
class Response extends Body {
  new(body: str);
  get status(): u16;
}
```

`Response` values are `externref`. `("tw:instanceof", "Response")` checks
them. Passing a `Response` where a `Body` is expected needs no conversion.

## 5. Modules and import modules

```ts
module "cloudflare:workers" {
  class DurableObject {
    new(ctx: DurableObjectState, env: Env);
    get ctx(): DurableObjectState;
  }
  export { DurableObject, DurableObject as default };
}
```

* A `module` block describes a JS module. Its export list names which
  declarations it exports, and under which names. A function renamed
  apart from its overloads ([§6](#6-members-and-keys)) is listed by its JS
  name, which names all of them.
* Imports key on the **declaration**, never on an export name: an export
  name resolves to the declaration slot (ecma262 ResolveExport). The bridge
  reaches the slot through any export that names it.
* A declaration the block doesn't export has no JS value, so it's
  reachable only through its instance members (`#`). It's a validation
  error for it to be a `class` (whose `("tw:instanceof", ...)` and result
  checks need the value), to have statics or constructors (other than
  `@js(object) new()` and `@js(new = ...)`), or to be a top-level member. The producer lowers
  such a type-only class as an `interface`.

### The import module field

A Wasm import is `(module, name)`. The name is the key ([§6](#6-members-and-keys)).
The module field is:

* the module specifier, for declarations inside a `module` block;
* `tw:js/global`, for ambient globals.

The IR's own identity never appears. Two `.d.ts` files declaring the global
`Request` describe the same JS object, so they produce the same import, and
the bridge dedups it.

Example:

| layer | import |
|---|---|
| language → bridge | `("cloudflare:workers", "DurableObject#get:ctx()")` |
| language → bridge | `("tw:js/global", "Request#get:url()")` |
| bridge → JS (ESM integration) | `("cloudflare:workers", "DurableObject")`, then a member get |
| bridge → JS (ESM integration) | `("tw:js/global", "Request")`, then a member get |

The module field is the same on both layers. The bridge imports declaration
values using the Wasm ESM integration convention, and performs member gets
and receiver calls itself. Under ESM integration something must resolve
`tw:js/global`: either the bridge generator emits a shim
(`export const { Request, fetch } = globalThis`), or a host provides it
natively.

### Lib IRs

```ts
ir 1;
lib std;

class Date {
  getTime(): f64;
}
```

```ts
ir 1;
with std;

interface Event {
  get timeStamp(): Date;
}
```

* `lib name;` makes an IR a lib: declarations that other IRs share instead
  of each declaring their own. `std` is the JS standard library, produced
  from TypeScript's `lib.*.d.ts`.
* `with name;` resolves the IR's paths against the lib `name` too. The IR's
  own declarations come first, then each `with` lib's, in order. Tools are
  handed the lib IRs (`--with std.ir`). A `with` that no supplied lib
  satisfies is a validation error.
* A lib's declarations keep their own location, import module and keys. A
  reference to a lib type adds no imports to the referencing IR: the lib's
  own bindings provide its members (as js-sys does for Rust), and the same
  key from two IRs is one import.
* A generic lib declaration takes the type arguments its lib declares.

## 6. Members and keys

### Member forms

| form | key | receiver |
|---|---|---|
| `f(...)` | `C#f(T,...)` | the instance |
| `static f(...)` | `C.f(T,...)` | none |
| `get p(): T` / `set p(v: T)` | `C#get:p()` / `C#set:p(T)` | the instance |
| `static get p()` / `static set p(v)` | `C.get:p()` / `C.set:p(T)` | none |
| `new(...)` | `C.new(T,...)` | none; the result is the enclosing type |
| `new name(...)` | `C.new:name(T,...)` | none |
| top-level `f(...)` | `f(T,...)`, `ns.f(T,...)` | none |
| top-level `get x(): T` / `set x(v)` | `get:x()`, `ns.set:x(T)` | none |

* A key is the declaration's full path, then `#` (instance) or `.`
  (static), then the member name (with `get:`, `set:` or `new:` for those
  forms), then the canonical parameter types in parentheses. Each
  parameter type is spelled from its reference, with aliases expanded up
  to their back-edges ([§3](#type-aliases)).
* Results and `throws` are not part of the key.
* `get`, `set` and `new` are mini-annotations: they tell generators what the
  member is, and generators may use or ignore them.
  * Kotlin folds a matching get/set pair into `var`, and takes the first
    setter when there are several.
* `new name(...)` exists for constructors whose keys would otherwise collide
  (hidden constants, §7).
* Top-level getters and setters are global values. Every read is live.
* `@js(object) new()`: the bridge creates `{}` instead of calling `new C()`.
  This is used for dictionary interfaces.
* `@js(new = a.b) new(...)`: the bridge runs `new a.b(...)`, constructing
  the JS value at that path instead of the class. The result is still the
  enclosing type. It's for constructor-shaped values
  (`declare const Wrap: { new <T>(value: T): Stub<T> }`), so it's allowed
  on an interface, and on a declaration a `module` block doesn't export
  (§5). The path is a JS value path, not an IR path: outside a `module`
  block its head is a global, and inside one its head is an export name.
  Construct signatures are overloads as usual. When a type is constructed
  through more than one value, the later ones are named
  (`@js(new = Wrap2) new wrap2(...)`).
* `@js(symbol = iterator) iterator()`: the member is the well-known symbol
  `Symbol.iterator`. It applies to methods, getters and setters, static or
  not (`@js(symbol = toStringTag) get toStringTag(): str;`). The IR name is
  the symbol's, renamed like any member when its key collides, and
  `symbol` excludes `name`. Only well-known symbols are supported.
* `@js(index)` marks producer-named element access (`at(i: u32)`,
  `setAt(i: u32, v: T)`) as JS indexing `o[i]`.
* A result of the enclosing type stands for TS `this` results.

### Generic members in keys

A mono-concretized variant prints its type arguments after the member name.
Parameters keep their declared spelling:

```
Map#get<Str,f64>(K)
```

When both the class and the member have type parameters, the class's
arguments come first, then the member's: `Array#map<f64,Str>(fn(T)->U)`.

Example:

```ts
class Headers {
  new();
  get(name: str): str?;
  set(name: str, value: str);
  static from(init: arr<(str, str)>): Headers;
}
```

| member | key |
|---|---|
| constructor | `Headers.new()` |
| `get` | `Headers#get(str)` |
| `set` | `Headers#set(str,str)` |
| `from` | `Headers.from(arr<(str,str)>)` |

## 7. Overloads and the `@js` call DSL

### Overloads

* Overloads are allowed. Each is keyed by its signature.
* A union parameter is one member with a `union` parameter (below), not
  an overload per arm.
* Trailing optional parameters become **arity overloads**. So a parameter's
  `?` only ever means nullable (non-trailing `T | undefined`).
* Keys can still collide after hidden constants are moved out (below). The
  producer then renames, **only on conflict**, following the Rust
  constant-expansion convention (CONVENTIONS.md: Variant names, and the
  `_with_<type>` vocabulary), recased to camelCase: `getText`,
  `set signalWithNull`.
  * A renamed member keeps its JS name through `@js(name = ...)`.
  * One shared helper in ts-gen-abi implements the convention. Generators
    without overloading apply the same helper to their own conflicts.

### `union` parameters

`x: union E` takes the payload enum `E`, and says every arm of `E` makes
the same JS call: only the argument conversion differs. A generator may
take `E` whole, or expand it into one overload per arm.

```
enum BodyInit { Text(str); Bytes(u8[]); Stream(ReadableStream); }
class Response {
  new(body: union BodyInit?);
}
```

* `union` is a parameter flag, not part of the type, so not part of
  keys: the member above is `Response#new(BodyInit?)`.
* It composes with nullability: `union E?`, `@js(null) union E?` and
  `union E??`.
* An enum parameter without `union` is a value passed explicitly, never
  expanded. A literal-only enum is always that, so it can't be `union`.
* **Derived arm variants.** Each `union` parameter, independently, can
  take one arm's payload type in place of `E`, keeping its `?`s: a lone
  payload is its type, several a tuple. Each such key is a valid import,
  `Response#new(str?)`, `Response#new(u8[]?)` and so on, generated only
  when a language imports it, as with mono instantiations (§13).

Validation:

* `union` applies to top-level import parameters, setters included: not
  nested types, results or export parameters.
* `E` is a payload enum whose arms the bridge can tell apart (§8): no two
  arms with the same test, such as two numbers, or two interfaces.
  Fallbacks and nested enums aren't compared.
* A real overload whose key equals a derived variant key must be the same
  JS call (kind, name and `@js`, parameters matched by position). It then
  stands in for the variant.

### `@js(args = (...))`

Constants are never IR parameters. `@js(args = (...))` is the JS argument
list, written in terms of the IR parameter names. It describes **call
shape** only. Value shapes are types, never attributes.

| element | meaning |
|---|---|
| `a` | the parameter `a` |
| `"text"`, `1`, `true`, `null` | a constant |
| `undefined` | padding |
| `...rest` | spread a sequence parameter |
| order | the order of the JS arguments (reordering is allowed) |

The default is the parameters in order. Each parameter appears **at most
once**, which makes every template invertible. A bare name is a parameter
or a `let`; `...rest` is the last element and spreads a sequence
(`arr<T>`, `T[]`). A view parameter (`*T[]`) is the typed array over the
memory, so `@js(args)` passes it directly.

**One template serves both directions.** The direction comes from the
declaration's role:

* a member import is outgoing (Wasm calls JS);
* an `export` is incoming (JS calls Wasm);
* a callback type takes its direction from the use site.

| element | outgoing (construct) | incoming (match) |
|---|---|---|
| `a` | supply `a` | bind `a` |
| constant | supply it | match by `===`, or the arm doesn't apply |
| `undefined` | supply it | match an absent argument or `undefined` |
| `...rest` | spread | bind the remaining arguments into the sequence |
| `p? { f = <expr>, ... }` | fill the object, creating it when absent; a field whose value is `undefined` isn't created | bind `p` to the argument, `null` when absent; each field matches its expression as a pattern on the object |
| `<expr>` | evaluate it | match its value by `===` |
| order | permute | permute back |

An incoming pattern is a parameter (which binds), a constant, or a constant
path through IR members (which match). Calls, `let`s and paths through
parameters can't be inverted, so on an export they're validation errors,
and so is a `let`.

**Incoming dispatch.** When several arms share one JS entry point, the
bridge picks one at runtime. It tests, in order:

1. arity;
2. constant patterns;
3. type tests, in declaration order.

With a single arm, a pattern mismatch throws `TypeError` **even with
validation disabled**: running the wrong arm would be incorrect, not merely
unvalidated. Outgoing calls never dispatch, since Wasm picks the overload
statically.

### Extended call shape

| entry | meaning |
|---|---|
| `this = p` | reserved: binding the JS receiver to a parameter is future work (§22), and validation rejects it |
| `super = (...)` | the `super(...)` arguments of an exported class constructor (§17); the default passes the constructor's arguments through |
| `let x = <expr>` | bind a value created by the bridge for this call |
| `p? { field = <expr>, ... }` | in `args`: set fields on a nullable object argument, creating it (`{}`) when it's absent. A field whose value is `undefined` isn't created, since a dictionary reads its own keys, so an absent optional parameter leaves its field absent |
| `cancel = <path>` | the function to call when the language drops the returned future (§16) |
| `$result` | predefined: the call's JS result, usable in `cancel` |

Expressions are parameters, `let` bindings, paths through either or
through IR members (`AbortController.new()`, `c.signal`, `c.abort`), and
constants. A field fill can take a parameter, so a flattened overload
builds its dictionary in one call: `args = (m? { from = from, text = text
})`. They're type-checked against IR
members, and no JS API gets special treatment.

### Other `@js` entries

* `name = "..."` renames, and only renames.
* `null` on a member is shorthand for `@js(null)` on its result's `T?`
  ([§9](#9-nullability)), or on the `T?` a `future<T?>` result resolves
  to: `@js(null) get s(): Str?` is `get s(): @js(null) Str?`. It's printed
  in the type position.
* `object`, `new = ...`, `symbol = ...` and `index` (§6).
* A string literal on an enum arm ([§8](#8-enums)).
* An error class match on an enum arm, with optional property predicates:
  `@js(DOMException[name = "NotFoundError"])` ([§15](#15-exceptions)).

**Parameter attributes** are written inline before the parameter:
`@js(null)`, `@js(unshared)` (§11) and `@once` ([§14](#14-handles)).
`@js(null) x: Str?` is shorthand for `x: @js(null) Str?`. `@once` needs a
callback parameter. `@js(null)` and `@once` apply only where the value
flows to JS, so on an export's own parameters they're validation errors.

Example:

```ts
class KVNamespace {
  @js(name = "get", args = (key, "text"))
  getText(key: str): future<str?>;
  @js(name = "get", args = (key, "json"))
  getJson(key: str): future<Any?>;
}
class RequestInit {
  @js(object) new();
  set signal(@js(null) value: AbortSignal?);
}
```

`kv.getText("a")` calls `kv.get("a", "text")`. If an export had the same
template, an incoming `get("a", "text")` would select that arm by matching
`"text"` with `===`.

## 8. Enums

```ts
enum Method {
  @js("GET") Get;
  @js("POST") Post;
  Latest = .Post;
}
enum Flags: u32 {
  Read = 1;
  Write = 2;
  ReadWrite = 3;
}
enum Body {
  Text(str);
  Bytes(u8[]);
  Stream(ReadableStream);
}
```

### Arms

| form | meaning |
|---|---|
| `Name;` | unit arm |
| `Name(T);`, `Name(A, B);` | payload arm |
| `Name = 1;` | numeric value in a carrier enum, otherwise an explicit index |
| `Name = .Other;` | alias: not counted as an index; resolves through bases |
| `@js("lit")` | the arm's JS literal (JS ABI only) |
| `@fallback` | catches anything unmatched; must be last |

### Wire shapes, derived from the arms

| arms | wire | JS |
|---|---|---|
| all unit, no carrier | `i32` index | the arm's `@js` literal |
| all unit, explicit values, `enum E: <int>` | the carrier type; **open set** (any value is legal, e.g. combinations) | the number |
| any payload arm | `(i32 arm, slots…)`, flattened | per arm |

* **An arm's index is implicit**: one more than the highest index before
  it in index order (base arms included), starting at 1, so a zeroed slot
  never decodes as an arm.
* **An arm may state its index**, `Name = 4;` or `Name(T) = 4;`, in an enum
  without a carrier. It's at least 1 and fits `i32`, and the arms after it
  continue from it. Indices exist so that bases can be combined (see
  Inheritance); an enum's discrimination order doesn't depend on them.
* **`@fallback` has index -1** wherever it is declared, takes no implicit
  index, and states none.

* In an enum with `: <int>`, `Name = 1;` is the arm's numeric value. The
  producer always writes the carrier of a numeric enum. Index enums are
  closed; their only openness is `@fallback`.
* In a carrier enum every arm has a value that fits the carrier, and no arm
  has a payload, a `@js` literal or `@fallback` (the set is already open).
* Every unit arm of an index enum has a `@js` literal, except
  `@fallback`. A missing literal is a validation error; the producer always
  emits them.

### Flattened payload enums

Each arm's payload flattens to a list of wire values. The enum's wire is
`i32 arm` followed by **slots**:

* Arms are assigned in **declaration order, base enums first**, the same
  order as indices, with each enum's `@fallback` in its declared position.
  Because of this, a base enum's slots are a prefix of a child's slots.
* For each wire value of an arm, the bridge reuses the first slot of the
  same wire type that this arm hasn't already used. If there's none, it
  appends a new slot.
* Numbers and references are never bit-cast into each other.
* Unused slots are zero or null, so a reference slot is always nullable
  (`ref null`), and a payload value matches slots by the nullable form of
  its wire type (a GC `(ref $str)` uses a `(ref null $str)` slot).
* Unit arms contribute no slots.

First-fit is normative. It never needs more slots than strict positional
matching, and gives the smallest wire for ordinary codegen.

The slot layout is a pure function of the declaration plus the profile.
One ts-gen-abi helper computes it, and the bridge generator and every
language generator use it.

**All discrimination happens in the bridge,** at the JS ABI, in one
crossing: it runs the arm tests, fills that arm's slots, and returns. The
language only matches on the arm index.

Arm tests are derived from the arm type:

| arm | test |
|---|---|
| unit with `@js("lit")` | `=== lit`, before any other test |
| class | `instanceof` |
| interface | non-null object |
| `str`, numbers, `bool`, `i64`/`u64`, std `Symbol` | `typeof` (plus the scalar validation) |
| `T[]` | `instanceof` the typed-array class |
| `arr<T>`, tuples | `Array.isArray` (plus length for tuples) |
| `fn(...)` | `typeof === "function"` |
| `future<T, E>` | thenable: `typeof v?.then === "function"` |
| nested enum | its own arms' tests, among the containing enum's (below) |
| box | the same test as its unboxed type (`Str` and `str` are both `typeof === "string"`) |
| `@fallback` | none |

A class path may hold no function at runtime, on `globalThis` or on its
module: workers-types declares `DurableObjectNamespace` as a class, but
workerd has no such global. The bridge resolves each class path once, at
instantiate time, and for a class with no value:

* a plain result or argument check degrades to the interface test (a
  non-null object);
* its arm test never matches, so discrimination falls through to later
  arms;
* `("tw:instanceof", ...)` returns 0.

Within each discrimination tier (the child's arms, then the base arms,
§8 Inheritance), arm tests run by kind, keeping declaration order within a
kind, so a structural test never catches a value a nominal one would:

1. unit arms with a `@js` literal;
2. nominal tests: classes, `T[]`, `arr<T>` and tuples (including an arm
   with several fields), `fn(...)`, `future<T, E>`;
3. structural tests: interfaces;
4. `typeof` primitives (and their boxes);
5. `Any` and type parameters.

A **nested enum** arm (one payload field, a non-nullable enum without a
carrier, and no `@js`) is tested as if flattened: its own arms (in its own
discrimination order, nested enums expanded in turn) take its place in the
containing tier, and the tier then orders all of them by kind as above,
keeping that order within a kind. A nested `@fallback` comes after the
tier's `Any` arms. The first arm that matches picks the nested arm holding
it and, inside it, that arm. A nested enum already being expanded (a
recursive union) is one structural arm, tested by its own discrimination.

So for `enum ZonedLikeOrDayLike { ZonedLike(ZonedLike); DayLike(DayLike); }`,
with `ZonedLike { Zoned(Zoned); ZonedFields(ZonedFields); Str(str); }` and
`DayLike { Day(Day); Zoned(Zoned); DayFields(DayFields); Str(str); }`, a JS
`Day` is `DayLike.Day`: the class `Day` is tested before the interface
`ZonedFields`, which takes any object. A plain object is
`ZonedLike.ZonedFields`, and a `Zoned` is `ZonedLike.Zoned`.

This order is separate from index order: `BodyInit`'s `Blob` arm (a class)
is tested before its `ArrayBufferView` arm (an interface) even when it's
declared after it.

Arm tests are dispatch, not validation, so they run even with validation
disabled. Tagged unions (`@js(when)`) are deferred.

Example: `Body` in the linear profile.

| arm | index | payload wire | slots used |
|---|---|---|---|
| `Text(str)` | 1 | `(i32, i32)` | 0, 1 |
| `Bytes(u8[])` | 2 | `(i32, i32)` | 0, 1 |
| `Stream(ReadableStream)` | 3 | `externref` | 2 |

The wire is `(i32 arm, i32, i32, externref)`. For JS
`new ReadableStream()`, the bridge returns `(3, 0, 0, stream)`. For
`"hi"`, it writes the UTF-8 bytes and returns `(1, ptr, 2, null)`.

### Inheritance

```ts
enum KvError extends err {
  @js(DOMException[name = "NotFoundError"]) NotFound(Str);
}
```

* A child **extends** its bases: it has more arms, so every base value is a
  child value. Base → child is free widening; child → base is checked
  narrowing.
* Index order: base arms first (the bases of `extends A, B` in order), then
  the child's arms. A base index keeps its value in the child.
  * A base reached twice (`extends A, B` where `B extends A`) is included
    once.
  * Two bases whose indices overlap can't be combined: that's an index
    collision, and a validation error. Implicit indices of two unrelated
    bases always overlap, so combining them needs explicit indices on the
    later one. The producer numbers the enums it combines apart, writing
    an index only where it differs from the implicit one:

    ```ts
    enum DateUnit { @js("year") Year; @js("month") Month; @js("day") Day; }
    enum TimeUnit { @js("hour") Hour = 4; @js("minute") Minute; @js("second") Second; }
    enum DateUnitOrTimeUnit extends DateUnit, TimeUnit {}
    ```
* Discrimination order is separate from index order: the child's arms are
  tested first, then base arms, then `@fallback`. So a child can catch a
  value before an inherited arm does.
* Arm names are unique across the hierarchy. There is at most one
  `@fallback`. Numeric values must not collide, and the carrier is
  inherited.
* Generators flatten the hierarchy, and may provide `From` (widen) and
  `TryFrom` (narrow).

For `KvError` the arms are `TypeError = 1`, `NotFound = 2`, `Error = -1`.
Discrimination tests `NotFound`, then `TypeError`, then `Error`.

### Fallback and no match

* `@fallback` must be the last arm, and it is taken unchecked.
* A unit fallback (`@fallback Unknown;`) catches unknown literals.
* A payload fallback keeps the value (`@fallback Other(Any);`).
* An incoming value that matches no arm, with no `@fallback`, is a
  validation failure (`TypeError`).

* Outgoing, a unit `@fallback` arm throws `TypeError`: the original literal
  is lost, so it has no JS value. A payload fallback (`@fallback Other(Str)`)
  is how unknown values round-trip.

### Nullable enums

* `Method?` is a plain `i32` with 0 meaning absent.
* A nullable payload enum uses `arm = 0` as absent.
* An explicit numeric enum (`Flags: u32?`, an open set where 0 may be legal)
  expands to `(i32 present, value)`.

`enum` + `namespace` merging folds the namespace's members into the enum's
statics, written `static` after the arms. They key as a class's statics do
(`Color.parse(str)`).

## 9. Nullability

* `T?` is the single absence. There's no `Null` type.
* `unit` has no value to cross (§3): "no result" (`future<unit>`), or a
  type argument (`Fetcher<unit>`). JS sees `undefined`.
* `Any` is non-null; `Any?` is its nullable form.

| direction | absence |
|---|---|
| JS → Wasm | `null` and `undefined` both become absence, always |
| Wasm → JS | `undefined` by default; `null` for a `T?` written `@js(null) T?` |

* `@js(null)` is an attribute of a `T?` node, at any depth (§3). JS → Wasm
  it changes nothing. The producer derives it from TS `| null`.
* A box's none is the profile's `box-none` (§12, §13): `ref.null` by
  default, which both JS empties enter as; or under `unit`, the unit box,
  which the language writes with `("tw:unit")`. A language never reads
  `@js`: at a `@js(null) T?` box node the bridge writes none as `null`.
* A null write is a `@js(null) T?` parameter:
  `set signal(value: @js(null) AbortSignal?)`. The Rust generator expands
  it into a `_with_null` arm, as wasm-bindgen can't pass `null` for an
  `Option`.
* **`T??` is the nested optional**, for three states (`x?: T | null`, `T
  | null | undefined`). Both directions, its outer absence is `undefined`
  and its inner absence `null`. `@js(null)` doesn't apply to it or to its
  inner `T?`, since the inner absence is already `null`.
  * `?` keeps its operand's box status. An unboxed `T??` lowers exactly as
    the payload enum `{ Undefined; Value(T?) }` (canonical path `T??`),
    through the standard flattening and bridge-side discrimination
    ([§8](#8-enums)): `Undefined` tests `=== undefined` first, and every
    other value is `Value`. There's no extra import.
  * A box `T??` (`Str??`) is a box: the JS value itself on one
    `externref`, with `null` as `ref.null` and `undefined` a non-null
    `externref`. A language writes its outer none as `tw:unit` and its
    inner none as `ref.null`. `("tw:unbox", str?)` reads an `undefined` as
    absent. This needs `box-none = unit`: under `null`, `??` is carried
    only by unboxed types, and a box `T??` lowers as `T?` with a warning.

* **The producer decides how many states a TS union keeps**, once, at the
  TS boundary. The IR always means what it says, so the bridge and every
  language honour a `T??` wherever it appears. `--strict-nullable` (off by
  default) keeps three states:

  | TypeScript | default | `--strict-nullable` |
  |---|---|---|
  | `T \| null` | `@js(null) T?` | `@js(null) T?` |
  | `T \| undefined`, `x?: T` | `T?` | `T?` |
  | `T \| null \| undefined`, `x?: T \| null` | `T?` | `T??` |

  `T | null` writes `null` in both modes, since TS rejects `undefined`
  there. By default, `T | null | undefined` writes `undefined`, which TS
  accepts, and its reads fold `null` into none. A lib IR is produced in
  one mode.

* `nullish` is a TypeScript-side tag, spelled `@tw-nullish`. It tells the
  producer to collapse a declaration's `null` into `undefined`, so reads
  and writes are plain `T?`.

Wire niches are in [§10](#10-primitives-and-validation) (scalars),
[§11](#11-sequences-and-memory) (sequences) and [§8](#8-enums) (enums).
Nullable references are `ref.null`.

Example:

```ts
class AbortSignal {}
interface RequestInit {
  get signal(): @js(null) AbortSignal?;
  set signal(value: @js(null) AbortSignal?);
}
```

* Reading `init.signal` when it's `null` or `undefined` gives the language
  `ref.null extern`.
* Writing `ref.null extern` sets `init.signal = null`.

## 10. Primitives and validation

### The JS representation follows the IR type

The bridge sees only the IR, so each type determines its JS value:

* `i53`/`u53` are a JS number, validated as a safe integer in range.
* `i64`/`u64` are a JS bigint. Wasm → JS always produces a bigint. JS →
  Wasm also accepts an integral number in range (any integral double
  converts exactly).
* `bigint` is a JS bigint of any size, and JS → Wasm accepts only a bigint.
  It crosses as its limbs ([§11](#11-sequences-and-memory)).
* Everything else is as in [§3](#3-types).

The producer maps:

* `number` tagged `i64`/`u64` → `i53`/`u53`;
* `bigint` tagged `i64`/`u64` → `i64`/`u64`;
* untagged `bigint` → `bigint`, and the lib's `BigInt` → its box `BigInt`;
* untagged `number` → `f64`.

### Carriers (padding)

Each type smaller than a Wasm value rides in its natural Wasm type, and
validation enforces the narrower range:

| type | wire |
|---|---|
| `bool`, `i8`, `u8`, `i16`, `u16`, `i32`, `u32` | `i32` |
| `i53`, `u53`, `i64`, `u64` | `i64` |
| `f16`, `f32` | `f32` |
| `f64` | `f64` |

### Nullable scalars

A nullable scalar never forces boxing:

| type | absence |
|---|---|
| `bool?` | `i32` 2 |
| `i8?` `u8?` `i16?` `u16?` | `i32::MIN` |
| `i53?` `u53?` | `i64::MIN` |
| `f16?` `f32?` `f64?`, `i32?` `u32?` `i64?` `u64?` | the pair `(i32 present, T)` |

Floats have no niche: every bit pattern, NaNs included, is a value. Enum
niches are in [§8](#8-enums).

### Validation

The boundary is untrusted.

* **Every validation failure throws `TypeError`**: a wrong `typeof`, a
  non-integer, or an out-of-range value alike.
* There are no implicit Wasm conversions (no wrapping, no `ToInt32`), and no
  `bool` coercion.
* A bridge option disables validation. Dispatch tests ([§7](#7-overloads-and-the-js-call-dsl),
  [§8](#8-enums)) still run. Unchecked values are coerced, never trusted,
  so the language still receives in-range carriers:
  * `bool` is JS truthiness;
  * `i8`, `u8`, `i16`, `u16` wrap into their range, and `i32`/`u32`
    wrap as `i32` (so `u8` 300 is 44);
  * floats are `Number(v)`, so NaN stays NaN;
  * `i53`, `u53`, `i64` and `u64` are a bigint as given, or
    `BigInt(Math.trunc(Number(v)))` (0 when not finite);
  * `str` is `String(v)`, and a non-typed-array `T[]` is empty.
* `strict-null = on` (a profile option, off by default) makes a `@js(null)
  T?` take only `null` as absence: JS → Wasm, `undefined` there is a
  `TypeError`, at any depth the bridge validates (a box position
  included). A plain `T?` still takes both, since a default-mode producer
  folds `T | null | undefined` into it (§9). Types, wire and language
  readings are unchanged.

Example: `setStatus(code: u16)` exported to JS.

| JS call | result |
|---|---|
| `setStatus(404)` | the language receives `i32` 404 |
| `setStatus(70000)` | `TypeError` |
| `setStatus(4.5)` | `TypeError` |
| `setStatus("404")` | `TypeError` |

For `size(): u53`, a JS result of `2 ** 53` is a `TypeError`. For
`id(): i64`, a JS result of `5` (a number) is accepted as `5n`.

## 11. Sequences and memory

### Sequence types

* `T[]` is the **typed array**, for the typed-array element types only, as
  a fresh copy in both directions, under every carrier (`host` included).
  The borrowed, no-copy form is `*T[]`:

  | `T` | `i8` | `u8` | `i16` | `u16` | `i32` | `u32` | `i64` | `u64` | `f16` | `f32` | `f64` |
  |---|---|---|---|---|---|---|---|---|---|---|---|
  | JS | `Int8Array` | `Uint8Array` | `Int16Array` | `Uint16Array` | `Int32Array` | `Uint32Array` | `BigInt64Array` | `BigUint64Array` | `Float16Array` | `Float32Array` | `Float64Array` |

  There's no widening: `bool[]`, `i53[]`, `str[]` and `(u32, str)[]` are
  validation errors, and use `arr<T>` instead. The box `Vec<T>` has the same
  restriction.
* `arr<T>` is a JS `Array` of any element type, as a fresh copy in both
  directions. Its box is `Arr<T>`.
* `bigint` is laid out as `u64[]`: its two's-complement little-endian
  `u64` limbs, `(ptr, len)` in linear memory and `(array (mut i64))` in GC.
  A writer gives the fewest limbs that keep the sign, at least one (`0` and
  `-1` are one limb, `2^63` two). A reader accepts any length, and zero
  limbs are `0`. Under the `host` sequence carrier it's the host bigint
  itself, as an `externref`.
* `(A, B, …)` is a fixed-length JS Array, validated with `Array.isArray` and
  `length`.
* `mut T[]` (parameters only) is **writeback**: the bridge copies in,
  makes the call, and copies back over the same storage.
* `*T[]` and `*mut T[]` (import parameters only) are zero-copy **views**
  ([below](#views-t-and-mut-t)).
* `Uint8Array`, `ArrayBuffer`, `DataView`, `Uint8ClampedArray` and the other
  buffer types are ordinary classes (externref). They convert to and from
  memory through `("tw:box", T[])` and `("tw:unbox", T[])`
  ([§13](#13-boxes-reference-wire-types-and-generics)), with no other
  protocol names:
  * `tw:box` returns a fresh typed array of `T`'s class, which is also a
    valid value of that class;
  * `tw:unbox` accepts any typed array of `T`'s class (`Uint8ClampedArray`
    for `u8`), and for `u8[]` also an `ArrayBuffer`, a `DataView` or any
    other view, read as its bytes.

### Tuples in direct positions

A tuple parameter or result flattens into multiple values, recursively.
A nullable tuple adds a leading `i32 present`. So `f(p: (u32, (f64, bool)))`
has the wire parameters `(i32, f64, i32)`.

### Linear-profile layout

| type | wire (memory32) | wire (memory64) |
|---|---|---|
| `str` | `(i32 ptr, i32 len)` | `(i64, i64)` |
| `T[]`, `mut T[]` | `(i32 ptr, i32 len)` | `(i64, i64)` |
| `*T[]`, `*mut T[]` | `(i32 ptr, i32 len)` | `(i64, i64)` |
| `arr<T>` | see below | |

* `len` counts elements (code units for `str`).

`arr<T>` splits each element's flattened wire values into two strided parts:

* **memory**: the numeric values, packed with no padding, so the stride is
  the sum of their sizes.
  * Scalars use their native size (`bool`/`i8`/`u8` 1, `f16` 2, `i53`/`u53`
    8).
  * Pointers and lengths are 4 bytes (8 on memory64).
  * A table base is 4 bytes.
  * Nested descriptors (`str`, `T[]`, nested `arr<T>`) store their own wire
    values. A plain `(ptr, len)` descriptor is 8 bytes on memory32 and 16 on
    memory64.
  * Access is unaligned: the bridge uses a `DataView`, and the language uses
    unaligned loads.
* **table**: the reference values, in the externref table, with a stride
  equal to the number of reference values per element. A reference that
  isn't an `externref` (a GC string, array or `anyref` handle under a mixed
  profile) is stored externalized with `extern.convert_any`, and read back
  with `any.convert_extern` and `ref.cast`. That includes a payload enum's
  GC reference slots; slots the element's arm doesn't use are zero or null.

The wire is `(ptr, len)` when every value is numeric, `(base, len)` when
every value is a reference, and `(ptr, base, len)` when mixed. Splitting
avoids boxing scalars, because Wasm can't read a number out of an
`externref` without a call.

* A nullable scalar in an `arr<T>` memory part is stored as `(u8 present, T)`,
  packed like every other value there. Scalar niches exist only on the call
  wire; nullable references are `ref.null` in the table. The same
  `u8 present` byte leads a nullable tuple and a nullable carrier enum.
* An enum stores its `i32` index or arm in 4 bytes, then its numeric slots
  at their wire size (4 or 8 bytes); its reference slots go to the table. A
  carrier enum stores its carrier at native size.
* `align` (for `tw:realloc`/`tw:free`) is the element's natural alignment:
  1 for UTF-8 `str` bytes (2 for WTF-16), `size_of(T)` for `T[]`, and
  for an `arr<T>` memory part the alignment of its largest field. Access inside
  a packed part stays unaligned.
* A zero-length sequence never allocates or frees, in either direction. The
  sender passes `ptr = align` (dangling, as Rust's `NonNull::dangling()`)
  and a table `base` of 0, and the receiver ignores both when `len = 0`.

Nullable `T[]?`, `arr<T>?` and `str?` use `len = u32::MAX` (`u64::MAX` on
memory64).

Example: `arr<(u32, str, Request)>`, linear, memory32. Each element has the
numeric values `u32, ptr, len` (12 bytes) and one reference. The wire is
`(ptr, base, len)`, where `ptr` points to `len × 12` bytes and `base` to
`len` table slots. Element *i*'s `Request` is in table slot `base + i`.

### Allocation and ownership

The language exports allocation in Rust's model (the Component Model's
`cabi_realloc` shape):

* `tw:realloc(old_ptr, old_size, align, new_size) -> ptr`;
* `tw:free(ptr, size, align)`.

The externref table has the hooks `tw:table/alloc(n) -> base` and
`tw:table/free(base, n)` (wasm-bindgen's `__externref_table_alloc` pattern).
The language defines and exports both the memory and the externref table,
by default as `memory` and `tw:table`. Both names are configurable, so a
bridge can share them with another binary. The table's index ranges are in
§14.

**Ownership:**

* **Results** transfer ownership to the caller.
  * Import results: the bridge allocates with `tw:realloc` and
    `tw:table/alloc`, and the language owns the result.
  * Export results: the bridge reads, converts, then frees with `tw:free`
    and `tw:table/free`.
* **Parameters** are borrowed for the call, and owned by the caller.
  * Outgoing (import parameters): the language owns them, and the bridge
    never frees them.
  * Incoming (export parameters): the bridge allocates, calls, and frees
    after the call, or on unwind ([§15](#15-exceptions)). The language must
    not retain them, nor rewrite the descriptors inside them, since the
    bridge frees nested allocations (an `arr<str>`'s strings) through
    those descriptors. Element data is the language's to change.
* `mut T[]` outgoing: the bridge copies the language's storage into a
  typed array, calls, then copies back into the same storage. The length is
  fixed.

### Views: `*T[]` and `*mut T[]`

A view is `(ptr, len)` into linear memory, like Rust's `*const [T]`, which
JS sees as a **zero-copy** typed array of `T`'s class over the memory. The
parameter is that typed array, so `@js(args)` passes it like any other:

```ts
class Crypto {
  getRandomValues(@js(unshared) buf: *mut u8[]);
}
class GPUQueue {
  @js(args = (buffer, offset, data))
  writeBuffer(buffer: GPUBuffer, offset: u53, data: *u8[]);
}
```

* It's always **unsafe**. Generators expose such members as unsafe (Rust
  `unsafe fn`), and producers usually add a safe `T[]`/`mut T[]` overload
  alongside.
* `*T[]` (read) vs `*mut T[]` (write) is generator intent. The bridge treats
  both the same.
* Views are created per call and never cached, since `memory.grow` detaches
  the buffer. A view is used only in `@js(args)`.
* Views are bounds-checked: `ptr + len × size_of(T) <= memory.byteLength`,
  or `TypeError`.
* Aliasing is the language's responsibility.
* `@js(unshared)` says the callee rejects a view of a `SharedArrayBuffer`
  (TS `Uint8Array<ArrayBuffer>`, a WebIDL buffer without `[AllowShared]`).
  If the memory's buffer is a `SharedArrayBuffer` at instantiation, the
  bridge passes a fresh copy instead, and writes it back after the call for
  `*mut T[]`. It applies only to views: every copy is a fresh `ArrayBuffer`.
* Views are available whenever a linear memory is configured, whatever the
  GC settings. They're legal only in import parameters; anywhere else
  they're a validation error.

Multiple memories are out of scope.

### GC-profile layout

| type | GC wire |
|---|---|
| `str` (`gc`, WTF-16, the default) | `(ref $str)`, `$str = (array (mut i16))` |
| `str` (`gc`, UTF-8) | `(ref $str)`, `$str = (array (mut i8))` |
| `str` (`host`) | `externref`, holding a validated JS string |
| `T[]` | `(ref (array (mut T')))`: `T'` is `i8` for `i8`/`u8`, `i16` for `i16`/`u16`/`f16` (holding the `f16` bits), `i32`, `i64`, `f32` or `f64` |
| `arr<T>` | `(ref (array (mut E)))`. For a scalar `T`, `E` is `T'` as for `T[]`, with `i8` for `bool` and `i64` for `i53`/`u53`, so `arr<u8>` and `u8[]` are both `(array (mut i8))`. Otherwise `E` is the element's wire value when there's one; a struct of the values (fields immutable) when there are several; `externref` for references. A reference `E` is always `ref null` |
| tuple element | a precise struct: `arr<(u32, str)>` = `(array (mut (ref null (struct (field i32) (field (ref $str))))))` |
| nullable sequence | `ref null` |

* Storage follows the element type only. `arr` vs `T[]` decides only the
  JS side (an `Array` or a typed array). A box element (`arr<U8>`) is an
  `externref`, the erasable form.
* Every GC carrier type is a singleton rec group, so the bridge's types and
  the language's types are identical by structural canonicalization.
* The GC profile needs no `realloc`. The bridge copies between JS and GC
  arrays through a private scratch memory, looping `array.get`/`array.set`
  with loads and stores (there's no bulk GC↔linear copy). WTF-16 strings
  use `wasm:js-string` `intoCharCodeArray` / `fromCharCodeArray`.
* Lowercase types use precise layouts and carry no runtime type
  information: the bridge always knows them statically.

* GC arrays are always `(mut E)`, and struct fields are always immutable.
  Both sides emit exactly these definitions, so the types are identical.
* A reference element is `ref null` even when its type is not nullable
  (`arr<str>` on the `gc` carrier is `(array (mut (ref null $str)))`), since
  `array.new_default` needs a defaultable element and most GC languages
  build arrays that way. The bridge checks a non-nullable element non-null
  when it reads one.

## 12. Profiles

The **profile** is shared by the bridge generator and the language
generator. The IR is unchanged by it.

| setting | values |
|---|---|
| `vec` (`T[]`) | `linear` \| `gc` \| `host` |
| `array` (`arr<T>`) | `linear` \| `gc` \| `host` |
| `str` | `linear` + encoding (UTF-8 by default, WTF-16) \| `gc` + encoding (WTF-16 by default, UTF-8) \| `host` |
| handles | `linear` (`i32`/`i64`) \| `gc` (`anyref`) \| `host` (`externref`) |
| linear memory | none \| `32` \| `64`; defined and exported by the language (default name `memory`) |
| externref table | defined and exported by the language (default name `tw:table`); alloc/free hook names |
| exception handling | `modern` (default) \| `legacy` |
| validation | on (default) \| off |
| strict-null | off (default) \| on: a `@js(null) T?` rejects `undefined` from JS (§10) |
| `box-none` | `null` (default) \| `unit`: an optional box's none in Wasm (§13) |
| termination flag | off (default) \| on |
| exported-class finalizers | on \| off |
| `results` | `multi` (default) \| `single` |
| `throws` | `tag` (default) \| `js` (§15) |
| protocol names | defaults (§19), each overridable |

Presets:

* **all-linear** (Rust): every category linear, memory32;
* **all-GC**: every category `gc`, with WTF-16 strings, which the
  `wasm:js-string` builtins read and write directly;
* **all-host** (Kotlin/Wasm): every category `host`, no memory.

Kotlin also needs `box-none = null`: it reads `undefined` as null, so it
can't hold the unit box.

GC doesn't preclude a linear memory: a language may be GC for every category
and still configure a memory (for `*T[]`), or turn GC off per category.
Without a memory there are no views, so generators skip `*T[]` members
(the producer keeps a safe `T[]` overload beside each).

With `host` strings, the language uses JS strings natively through
`wasm:js-string`. There `str` and `Str` share the wire type `externref`;
`str` only adds the static guarantee that the value has been validated.

With a `host` carrier, the value crosses as one `externref` holding the JS
value itself, for languages whose interop can name no GC types (Kotlin/Wasm
externals take only primitives and `externref`):

* `T[]` is a typed array, and the bridge copies it in both directions
  rather than hand over the caller's, so neither side sees the other's
  later writes. `mut T[]` passes the caller's own typed array, so writes
  land in it. The no-copy form is `*T[]`, which needs a memory.
* `arr<T>` is a JS array whose elements are the JS values of `T`. Each
  crossing is a fresh value, so the receiver owns it.
* A handle is the language object externalized (`extern.convert_any`),
  which the bridge treats as opaque. As in GC, no finalizers are needed.

**memory64:** pointers, lengths, handles (linear) and the `tw:realloc` /
`tw:free` arguments become `i64`, descriptors become 16 bytes, and the niche
becomes `len = u64::MAX`. Table indices stay `i32`.

The handle carrier is a per-category setting like the others, so a language
can be GC for every category or turn GC off for handles alone.

### `results = single`

A capability fallback for languages that can't declare multi-value
functions (Kotlin/Wasm), not a performance option: `multi` is the fast
path, since the bridge returns a JS import's extra results through a
scratch return area. It applies to every core function between the
language and the bridge whose flattened results are more than one value,
in both directions: IR imports
and exports, callback trampolines and calls, and the `tw` protocol
operations (`tw:future` `new` and `take<T>`, `tw:unbox`, `tw:arr get<T>`,
`tw:err`). Multi-value parameters are unaffected.

* The language declares slot 0 as the only result. Slots `1..n` go through
  the **spill**.
* **The language calls an import:** the import returns slot 0 with slots
  `1..n` on the spill. Straight after the call, the language pops each with
  `("tw:ret", "<vt>")() -> <vt>`, in slot order.
* **The bridge calls a language export:** the language pushes slots `1..n`
  in slot order with `("tw:ret", "push:<vt>")(v)`, then returns slot 0. The
  bridge drains the spill right after the call.
* `<vt>` is `i32`, `i64`, `f32`, `f64` or `externref`. A slot of another
  reference type (a GC carrier) can't spill, so its member is unsupported
  under `single`.

Order is slot order both ways: from the language's side the spill is first
in, first out within a crossing. It's empty at every crossing boundary:

* the bridge checks it's empty before each spilled import and around each
  export;
* reading a slot that wasn't pushed, overflowing, or leaving slots unread
  throws a `TypeError` and resets it, as does any exception unwinding out of
  an export.

Storage is the bridge's: `externref` slots in the `tw:table` range
`[stack_base, const_base)` (§14), growing down from `const_base`, and numbers
in 8-byte slots of the bridge's scratch memory. So under `single` the bridge
defines `tw:table` and its base globals itself, and the language must not
export one. Each crossing's spill fits in 127 references and 64 numbers.

## 13. Boxes, reference wire types and generics

### Boxes

A box (`Bool`, `I8` … `F64`, `Str`, `BigInt`, `Unit`, `Vec<T>`, `Arr<T>`, `Tuple<A, B>`) is
an **opaque value of the bridge**. It's a single `externref` in every
profile, it is never converted, and its parameters name unboxed inner types
(`Vec<u8>`, `Tuple<str, u16>`).

* The language never builds or inspects a box. The bridge does it through
  derived operations:
  * `("tw:box", T)`: `(T wire) -> externref`. It copies out, for example
    into a JS array.
  * `("tw:unbox", T)`: `(externref) -> T wire`. It runs a type test and
    throws `TypeError` on mismatch, then copies into the language's carrier
    and validates. `tw:unbox T?` reads both `undefined` and `null` as none.
  * `("tw:unboxUnchecked", T)`: the same, without the type test and
    validation, for a language that trusts the declared type. It still
    normalizes none, and stays memory safe, since the bridge's own
    operations (for example, `wasm:js-string`) trap on wrong types.
  * `("tw:isNullish")` and `("tw:isUndefined")`, with an empty name:
    `(externref) -> i32`, since Wasm can't compare externrefs. A `T?` box's
    presence test is `tw:isNullish`. A `T??` box's outer none is
    `tw:isUndefined`, and its inner none is `ref.is_null`.
  * `("tw:unit")`, `("tw:true")` and `("tw:false")`, with an empty name:
    `() -> externref`, the unit box (`undefined`) and the `Bool` boxes. The
    bridge module
    serves them, so the call is Wasm to Wasm. They're the portable form of
    the externref table's constants (§14), and the two are equivalent: a
    language that reaches the table reads `table.get(const_base)`, `+ 2` and
    `+ 3` instead, the fast path.
* **A box is an ABI value with a normative representation.** The bridge
  does every conversion, so the Wasm ABI stays pure:
  * the host aggregates (`Str`, `Arr<T>`, `Vec<T>`, `Tuple<…>`) and class
    references are encapsulated host references, operated on only through
    imports;
  * a number box (`I8` … `U32`, `I53`, `F16` … `F64`) in an `anyref`
    position is an `i31ref` when its value is integral and fits (by value,
    not by type), and the host number otherwise. A language reads one with
    `i31.get_s`/`i31.get_u` and makes one with `ref.i31`, with no call;
  * `I64` and `U64` are the host bigint;
  * `Bool` is the host boolean, never an `i31ref`: in an erased position the
    bridge has no type to read one with;
  * `Unit` is the host `undefined`, and `ref.null` under
    `box-none = null`.

  The language knows every box's type, but the bridge knows it only in mono
  positions, so an erased box describes itself, and only numbers share
  `i31ref`.
* **Boxes are lazy**: a box is the faithful host value, and every check
  waits for `tw:unbox`. Nothing is tested when a box crosses a position,
  so a non-null `Str` position passes `undefined` through, and
  `tw:unbox str` then throws. Only none is normalized, as `box-none` says
  (below). `("tw:box", T?)` writes none as the box's none. Host containers
  stay raw: an `Arr<T?>` element or a `Tuple` field keeps its empty, and
  `tw:arr get<B?>` normalizes it as `tw:unbox` does. The JS API's
  conversion to `anyref` gives the `i31ref` form.
* **A box's none is the profile's `box-none`** (§12). A language never
  reads `@js` annotations: the bridge applies them.
  * `null` (default): an optional box's none is `ref.null` everywhere in
    Wasm. On entry, the bridge maps `undefined` and `null` to `ref.null` at
    optional box nodes and erased positions; a non-optional box node stays
    faithful. On exit, `ref.null` at a typed node writes its default
    (`undefined`, or `null` under `@js(null)`), and an erased position
    writes `undefined`. `tw:unit` is only a value constant, read as none.
    The loss is empties through erased positions: an `Any` `null` read
    through an erased `Arr<T>` goes back out as `undefined`.
  * `unit`: a language writes an optional box's none as the unit box. At a
    concrete `@js(null) T?` node, the bridge writes a box holding unit as
    `null`, by an `=== undefined` test that needs no `T`. Everything else
    is faithful: default nodes, erased type arguments, and received boxes
    passed back (a received `null` goes back as `null`). A `T??` box's
    outer none is unit and its inner none `ref.null`: that's type
    semantics, not a JS annotation. `null` is `ref.null`.
* A box's host representation, its JS binding:

  | box | host value |
  |---|---|
  | `Unit` | `undefined` |
  | `Bool` | boolean |
  | `I8` … `U32`, `I53`, `U53`, `F16` … `F64` | number; an `i31ref` in `anyref` positions when integral, in [−2³⁰, 2³⁰) and not −0 |
  | `I64`, `U64`, `BigInt` | bigint |
  | `Str` | string |
  | `Vec<T>` | the typed array of `T`'s class |
  | `Arr<T>` | an Array of `box(T)` |
  | `Tuple<A, …>` | a length-n Array of `box(A)`, … |
  | an enum `E` | its arm's host value: a unit arm's `@js` literal, a carrier enum's number, a payload arm's `box(payload)`, `@fallback`'s raw value |
  | a class, interface, `Any`, `Symbol` or JS function | itself |
  | `future<T>` | a Promise of `box(T)` |
  | an exported class, a callback the language provides | its wrapper; `tw:unbox` recovers the handle |
* Boxes are **values**, not classes:
  * `Vec<u8>` has no identity, and may be copied;
  * `Uint8Array` is an object with identity and an API.

  They stay distinct types even though both are externrefs.
* Scalar boxes are legal in any position.

`Arr<T>` boxes can be read without a full copy through the `tw:arr` group:

| import | signature | meaning |
|---|---|---|
| `("tw:arr", "len")` | `(externref) -> i32` | the length, as `u32` |
| `("tw:arr", "get<T>")` | `(externref, i32 index) -> W(T)` | one element, converted and validated as `("tw:unbox", T)`; an out-of-range index is a `TypeError` |
| `("tw:arr", "get<B>")` | `(externref, i32 index) -> externref` | for a box element type `B` (`get<Str>`, `get<Arr<u8>>`), the element itself as a box, unconverted; `get<B?>` reads an empty as `ref.null` |

More operations are future work.

Unboxing in the GC profile: a JS string goes into `$str` with
`intoCharCodeArray`. An `arr<T>` goes into a precise GC array.

### Reference wire types

A reference's wire type follows what the value **can be**:

| the value can be | linear | GC |
|---|---|---|
| only a host value: classes, interfaces, `Any`, boxes, `future`, a JS function received | `externref` | `externref` |
| a language object: handles ([§14](#14-handles)) | `i32` (`i64` on memory64) | `anyref` |
| erased `T`: a box, a host reference or a wrapper | `externref` | `anyref` |

The linear and GC columns of handles and erased `T` follow the handle
carrier setting ([§12](#12-profiles)). With `host` handles, both are
`externref`, and a language object crosses externalized. Erased `T` is
`anyref` in GC for its `i31ref` number boxes. It never holds a language
object: a handle enters an erased position only through `("tw:box", C)`, as
its wrapper (below).

* `Cache#put(Request,Response)` has the same wire, `(externref externref
  externref)`, in both profiles.
* From an erased (`anyref`) position, the language converts to `externref`
  with `extern.convert_any` before calling `("tw:unbox", T)`. The JS API
  maps an externalized `i31` back to a number.

In every profile, a language handle enters an erased position only as its
JS wrapper ([§14](#14-handles)): `("tw:box", C)` makes or reuses the wrapper
(only reuses it for a class with an imported base, §17), and
`("tw:unbox", C)` returns the handle (a `TypeError` if the value isn't a
live wrapper of `C`). This is symmetric with boxes. With validation on
(§12), the bridge rejects a raw language object reaching JS from an erased
position with a `TypeError`. The same
holds for a callback type `F`. `("tw:tryUnbox", C)` is `(externref) ->
W(C?)`: the handle, or none when the value isn't a live wrapper of `C`,
without an exception. It takes only a class with handles or a function
type.

### Mono generics (concretized, usage-driven)

The bridge is generated **after** the language module is compiled, so it
derives each concrete variant from the language's imports, and never
enumerates them.

* A concrete variant carries its type arguments in the key (§6):
  `Map#get<Str,f64>(K)`. The bridge substitutes them into the generic
  signature and reuses the per-type converters.
* **Concrete bounds** `<K in (Str, f64)>` give a finite set of allowed
  concretizations. The bridge rejects any other. Languages without
  per-instantiation import names can enumerate the set as overloads. With
  no bound, any type is allowed.
* **Exports** work the same way in reverse: a generic export the language
  exports under its erased key is erased; otherwise each mono variant it
  exports (`util.id<u32>(T)`) is an overload of the JS member, dispatched
  on its arguments in the bound's order. Variants the arguments can't tell
  apart (`make<T>(): T`) are an error, and so are mono variants of a generic
  exported class's members, since JS can't name an instance's type
  arguments.
* Generators may take an instantiation list
  (`--instantiate 'Map<Str,f64>'`).
* Zena: `@external` names templated per instantiation
  (`@external("tw:js/global", "Map#get<{K},{V}>(K)")`).

### Erased generics

* There's exactly one ABI per generic member. Every `T` is a box, a host
  reference or a handle's wrapper, carried as `anyref` in GC and
  `externref` in linear. An enum crosses as its box, through `("tw:box",
  E)` and `("tw:unbox", E)` (`Map<Str, Mode>`).
* The box representation (above) lets a language skip a call where it's
  identical by definition:
  * a number box in an erased (`anyref`) position that is an `i31ref` is
    read and made directly, and `tw:unbox` of any number box type accepts
    one whose value fits it (an `F64` box holding `3` is an `i31ref`);
  * under the `host` string carrier, `Str` ≡ `str`;
  * a `Bool` box is the table's `true` or `false` constant, or
    `("tw:true")`/`("tw:false")` in the bridge module, with no JS call;
  * JS class references aren't boxes, and pass through erased positions
    as-is. A language handle goes through `tw:box` and `tw:unbox` (above).
* The cost is one bridge call per box/unbox, on erased paths only.
  Converting to a JS value has to happen anyway.

### Bounds

* `T: C` is a subtype bound (TS `<T extends C>`).
* `T in (A, B)` is a concrete bound: a finite set of allowed
  concretizations. A one-element set is `T in (A)`.

The IR has no generics mode flag.

### Defaults

A type parameter may have a default, after its bound: `T = D`, `T: C = D`,
`T in (A, B) = A`.

```ts
interface Fetcher<T = unit> {}
class D1PreparedStatement {
  all<T = Record<Any?>>(): future<D1Result<T>>;
}
```

* The default is the **erased type**. A generator that erases reads an
  erased or uninferrable parameter as its default, else its bound, else
  `Any`. A mono generator uses it for an argument the language leaves out.
* A type reference still gives every argument, so keys always carry the
  full list: the producer writes defaults out (`Fetcher<unit>`).
* A default may name the parameters before it. An `in` bound's default is
  one of its elements.

Example: Kotlin, erased, `Map<Str, Tuple<u32, bool>>`.

1. `map.set("a", Pair(7u, true))`: Kotlin calls `("tw:box", "Str")` and
   `("tw:box", "(u32,bool)")`, then `("tw:js/global", "Map#set(K,V)")`
   with `(externref, anyref, anyref)`.
2. `map.get("a")` returns `anyref` holding the JS array `[7, true]`,
   unconverted.
3. Kotlin knows `V` statically, and calls `("tw:unbox", "(u32,bool)")`,
   which validates and returns `(i32 7, i32 1)`.

### Associated types

A type can vary with a concretely bounded parameter by more than
substitution: TS `PluralizeUnit<Units>` is a different enum for each unit
set. An **associated type** tabulates it, one row per element of the bound:

```ts
interface RoundingOptions<Units in (AnyUnit, DateUnit, TimeUnit)> {
  type PluralUnit = match Units { AnyUnit => PluralAnyUnit, DateUnit => PluralDateUnit, TimeUnit => PluralTimeUnit };
  get smallestUnit(): PluralUnit?;
  set smallestUnit(value: PluralUnit);
}
```

* `match` names a type parameter of the class or interface that has an
  `in` bound. The rows are exhaustive: one per element of the bound, in
  the bound's order.
* A parameter passed on to another declaration (`extends
  RoundingOptions<Units>`) instantiates it at each element of its own
  bound, so that declaration's bound holds them too.
* Inside the declaration, the associated type is a type that takes no
  arguments. Its name is unique among the declaration's type parameters
  and associated types, and a row can't name an associated type.
* A **mono** consumer substitutes the row for the argument:
  `RoundingOptions<DateUnit>`'s `smallestUnit` is `PluralDateUnit?`.
* An **erased** consumer reads the first row. The producer puts the
  parameter's own constraint, the union of the bound (`AnyUnit`), first.
* Keys spell an associated type by its name, like a type parameter, and
  a mono variant's type arguments are the real parameters' only:
  `RoundingOptions#set:smallestUnit<DateUnit>(PluralUnit)`.
* Rust generators can emit a trait with the associated type and one impl
  per row.

## 14. Handles

A **handle** is a language-owned value held by JS: a callback's environment,
or an instance of an exported class. Its wire type follows the profile:
`i32` (a pointer; `i64` on memory64) in linear, `anyref` in GC, and the
externalized object (`externref`) with `host` handles. A nullable
handle (`Counter?`) is `0` in linear, since handles are non-null pointers,
and `ref.null` in GC.

* The bridge wraps a handle in a JS value (a function, or an exported-class
  instance). **Pairing:** the same handle yields the same JS value while it's
  alive, so `removeEventListener` works. In a typed position the bridge
  wraps a handle itself; into an erased one, the language boxes it with
  `("tw:box", C|F)` first ([§13](#13-boxes-reference-wire-types-and-generics)).
* A wrapper is made when its handle first crosses to JS, and made again if
  it was collected while the language kept the handle, since a wrapper with
  no state of its own can be recreated unobservably. That holds for every
  callback and for a base-less exported class. An exported class with an
  imported base is the exception: its wrapper comes only from `tw:new`
  ([§17](#17-exports)).
* **Finalizers (linear only):** the bridge registers each wrapper with a
  `FinalizationRegistry`. Collecting the wrapper calls the language's drop
  export (`tw:drop:fn(Event)`, `tw:drop:Counter`). In GC the wrapper and the
  language object are collected by the same GC: no finalizer, no drop
  export.
  * For exported classes, finalizers are a profile option.
* **Language-side end of life:** the language calls `("tw:destroy", F)` or
  `("tw:destroy", C)`, of type `(handle)`. The bridge then:
  1. marks the wrapper dead, so later calls throw `TypeError` without
     reaching the language (no use-after-free);
  2. unregisters the finalizer (no double free);
  3. removes the pairing entry, so a reused pointer gets a fresh wrapper
     (from `tw:new` for a class instance with an imported base, §17).

  The wrapper stays a valid JS value. In GC, `destroy` revokes the wrapper
  early but frees nothing.
* **`@once`** (a semantic attribute on a callback-typed parameter) enforces
  at most one call: the bridge destroys the wrapper automatically after the
  first call. It gives no at-least-once guarantee; the finalizer still
  applies in linear. Anything that needs completion uses `future`.

`@once` is written inline before the parameter:
`then(@once cb: fn(Response))`.

Example: Rust, linear.

```rust
let cb = Closure::new(move |e: Event| count.set(count.get() + 1));
target.add_event_listener("click", &cb);
drop(cb);
```

* `add_event_listener` passes the handle (`i32` `0x1a40`). The bridge makes
  or reuses the JS function and registers it.
* `drop(cb)` calls `("tw:destroy", "fn(Event)")(0x1a40)`.
* A later click throws `TypeError` from the dead function, and the language
  is never entered.

### The externref table

The externref table is split into ranges, wasm-bindgen heap style:

```text
0 .. stack_base .. const_base .. heap_base ..
```

| range | holds |
|---|---|
| `[0, stack_base)` | reserved; slot 0 is null |
| `[stack_base, const_base)` | the spill (§12 `results = single`), growing down from `const_base` |
| `[const_base, const_base + 4)` | the constants `undefined`, `null`, `true`, `false`, in that order |
| `[const_base + 4, heap_base)` | reserved |
| `[heap_base, ..)` | the slots `tw:table/alloc` hands out |

* Whichever side defines `tw:table` also exports the three bases as
  immutable `i32` globals: `tw:table/stack_base`, `tw:table/const_base` and
  `tw:table/heap_base`. Tables are 32-bit indexed, so they're `i32` under
  memory64 too. They need `0 < stack_base <= const_base` and
  `const_base + 4 <= heap_base`.
* The bridge reads the bases at instantiation, grows the table to at least
  `heap_base`, and stores the constants. They're valid from the first
  entry into the language, not during its start function.
* The constants are the index form of `("tw:unit")`, `("tw:true")` and
  `("tw:false")` (§13), and equivalent to them: `table.get(const_base)` is
  the unit box. They're the fast path for a language that reaches the
  table; the function imports are the portable form.
* When the language doesn't export a table, the bridge defines `tw:table`
  and its globals with the defaults `1`, `128` and `132`.

## 15. Exceptions

### Transport and throw levels

Exceptions travel through native Wasm exception handling. Both modern and
legacy EH are supported, as a profile setting.

| declaration | meaning |
|---|---|
| no `throws` | no error type: an exception **may unwind** through the language, up to the entry point's caller |
| `throws` | the built-in enum `err` |
| `throws E` | the enum `E` |

### Tags

Any enum used in `throws` derives a dedicated Wasm tag, `tag:<E>`. The tag
carries the enum's flattened value `(i32 arm, slots…)`, the same shape as in
every other position. There's no global numbering of errors.

The bridge defines every tag and provides it as the import
`("tw:tag", "<E>")`; the built-in is `("tw:tag", "err")`. Languages import
the tags they use.

An enum used in `throws` must:

* **end in `@fallback`** (its own, or inherited);
* have only `(Str)` payload arms, for now.

So its tag is `(i32, externref)` in every profile.

### The built-in `err`

```ts builtin
enum err {
  @js(TypeError | RangeError) TypeError(Str);
  @fallback Error(Str);
}
```

* `@js(A | B)` matches either class incoming, and constructs the first one
  outgoing. So a host `RangeError` round-trips as a `TypeError`, which is
  accepted.
* Specific enums extend it and inherit both arms, including the fallback.
* It's **minimal and frozen**: adding base arms would shift every child's
  indices.
* `future<T>` means `future<T, err>`.

### Mapping

Arms map to JS errors through `@js(<class>[<prop> = <lit>, …] | …)`. The
bracketed property predicates are optional. A class path that resolves to
an IR class uses that class's JS value; any other path names a JS global
(`TypeError`, `DOMException`), which is how the built-in `err` refers to
the standard error classes.

* **Incoming (JS → tag):** the bridge tests arms in discrimination order
  ([§8](#8-enums)): `value instanceof <class>` for one of the classes, then
  that class's predicates (`value.name === "NotFoundError"`). An arm with no
  `@js` never matches, except `@fallback`, which matches anything.
* **Message extraction** for a `(Str)` arm: `err.message || String(err) ||
  'unknown error'`. Reading `message` and calling `String(err)` both run
  user code and can throw (a throwing getter or `toString`), so each is
  guarded: a throwing `message` counts as no message and falls through to
  `String(err)`, and a throwing `String(err)` falls back to
  `'unknown error'`. The message is a boxed `Str`: no allocation on the
  error path, and it's unboxed lazily.
* **Outgoing (tag → JS):** the bridge constructs the arm's first class with
  the message (`new TypeError(message)`). An arm with no `@js` constructs
  `new Error(message)`.
  * `DOMException` takes its `name` predicate as the constructor's second
    argument: `new DOMException(message, name)`.
  * For every other predicate the new instance doesn't already satisfy, the
    bridge defines an own data property (`Object.defineProperty`), since
    such properties are often read-only accessors on the prototype.

### `throws = js`

A profile setting for languages that can't import tags (Kotlin/Wasm), with
or without `results = single`. The bridge uses no tags with the language,
and `("tw:tag", E)` imports are an error.

* **Imports with `throws E`:** the bridge rethrows the original JS value
  untagged, so the language catches it as a JS exception. It then reads it
  with `("tw:err", "E")(exn: externref) -> (i32 arm, slots…)`, which maps it
  like a tag would ([Mapping](#mapping)). Validation `TypeError`s propagate
  the same way and map to the `TypeError` arm.
* **Exports and trampolines with `throws E`:** the language doesn't throw.
  It calls `("tw:err", "set:E")(arm, slots…)` and returns normally, with
  zero values and nothing pushed. The bridge sees the pending error, drops
  the results and throws the arm's outgoing JS error.
* Futures are unchanged: `reject<E>` and `error<E>` carry the enum's value
  as before.

### Where mapping happens

* **Imports with `throws`:** the bridge catches under `JSTag`, maps the
  error, and throws `tag:<E>` into the language.
* **Entry points** (exports, exported-class methods, callback trampolines):
  the bridge catches every known tag, maps it outgoing, and throws the JS
  error. Incoming argument validation throws real JS `TypeError`s.
* Validation failures get no special treatment: inside an import with
  `throws`, a validation failure is a `TypeError`, discriminated like any
  caught error. With `err`, it lands on `TypeError`.
* **Unknown exceptions pass through untouched:**
  * a JS exception that unwound from an import with no `throws` is rethrown
    as the original value, so identity is preserved;
  * a foreign tag (a Rust panic, a C++ exception) leaves as a
    `WebAssembly.Exception`. Converting panics is the language's job.
* Identity loss on mapped round trips is accepted: a mapped JS error comes
  back reconstructed (same class and message, a new object).

### Unwinding without `throws`

The language defines what an unwinding JS exception means.

* In Rust it unwinds as an uncatchable panic and runs destructors. It's
  fatal under `panic=abort` and recoverable under `panic=unwind`.
* GC languages define their own rule.

**Termination flag** (profile option; recommended for Rust under
`panic=abort`):

* The language exports `tw:terminated` (0 live, 1 terminated), and the
  language or JS sets it.
* The bridge checks it on **every entry**: exports, exported-class methods,
  trampolines, finalizers, `tw:loop/tick` and `tw:future/wake`. When it's
  set, the bridge throws `"Module terminated"`.

`tw:terminated` is an exported global, and the bridge reads it according to
the profile:

* with a linear memory: an immutable pointer-sized global (`i32`, or `i64`
  on memory64) holding the address of a state byte (wasm-bindgen's
  `__instance_terminated` shape);
* GC with no memory: a mutable global holding the state itself.

### Bridge unwind and memory safety

The bridge is **unwind safe** and **fully memory safe**, including through
validation failures.

* Every bridge wrapper around a call into Wasm installs cleanup handlers:
  * modern EH: `try_table` with `catch_all_ref`, then cleanup, then
    `throw_ref`;
  * legacy EH: `try` / `catch_all` / `rethrow`.

  The cleanup releases incoming argument buffers (`tw:realloc`), table slots
  (`tw:table/alloc`), scratch use, and per-call `@js` state. Then it
  rethrows, or maps the exception if it's a tag.
* **Conversions are transactional.** Every allocation made while converting
  a value goes onto a per-call cleanup list, which the JS glue and the Wasm
  wrapper share.
  * A failure part-way through (a `TypeError` at element 500 of a list, a
    throwing getter or Proxy trap) releases everything allocated so far,
    including nested strings and slots, before the error propagates.
  * Ownership of a result passes to the language only on success.
* User JS can run during a conversion (getters, Proxy traps), and may
  reenter the language. That's ordinary reentrancy: partially built values
  are never visible to the language.
* Traps skip cleanup. That's acceptable, since the termination flag poisons
  the instance.

Example:

```ts
enum KvError extends err {
  @js(DOMException[name = "NotFoundError"]) NotFound(Str);
}
class Kv {
  get(key: str): str throws KvError;
}
```

1. JS throws `new DOMException("missing", "NotFoundError")`.
2. The bridge throws `tag:KvError` with `(2, "missing")` into the language.
3. The language lets it escape an export.
4. JS receives `new DOMException("missing", "NotFoundError")` (a new
   object).
5. `throw 42` from JS gives `(-1, "42")`: the `Error` arm.

## 16. Async

### `future<T, E>`

* `future<T, E>` replaces `Promise<T>`: `T` is the result, `E` the
  rejection enum. `future<T>` is `future<T, err>`. `future<never>` only
  rejects (§3).
* A future is an `externref` (a JS Promise).
* There's no async syntax and no JSPI. Languages treat functions that
  return a `future` as async on their own.
* **Folding:** without `throws`, every synchronous exception (validation or
  host) becomes the future's rejection (WebIDL's rule). With `throws`,
  synchronous throws go to the tag.

* An export that returns a `future` folds its own argument validation
  failures into the returned Promise's rejection too, instead of throwing
  synchronously (WebIDL's behaviour).

### The cooperative loop (tokio-rs/tokio#8479)

The ambient JS event loop is the only parent. Fairness is its FIFO
interleaving of macrotasks.

* **The language creates its own loops** (one per request, as in
  workers-rs) through its bindings' macro system. Loop and waker handles
  are `i32`s owned by the language.
* The language exports:
  * `tw:loop/tick(loop: i32, max: u32) -> u32`: runs up to `max` ready
    tasks, and returns non-zero if ready work remains. The bridge passes a
    budget of 64;
  * `tw:loop/next_sleep(loop: i32) -> u53?`: in nanoseconds. 0 means work
    is ready now; absence means nothing is scheduled;
  * `tw:future/wake(waker: i32)`: a remote wake, routed by the language to
    the waker's loop.
* The bridge provides:
  * `("tw:loop", "work_available")(loop)`: posts one coalesced macrotask
    that ticks that loop. It never reenters;
  * `("tw:loop", "drop")(loop)`: clears that loop's pending macrotask or
    timer.
* **Driving:** a tick runs in its own macrotask. A non-zero tick result
  posts another macrotask. Otherwise the bridge reads `next_sleep`:
  * 0: post another macrotask (`MessageChannel`);
  * `n`: `setTimeout`;
  * absent: stay idle.
* A promise's `then` callback (a microtask, which can never interleave
  inside a tick) calls `tw:future/wake` directly. That's tokio's remote-wake
  path.
* Multiple bridges and loops coexist in one JS event loop: all state is per
  instance, with no globals.

### Future operations (bridge imports, group `tw:future`)

| name | signature | meaning |
|---|---|---|
| `poll` | `(externref f, i32 waker) -> i32` | 0 pending, 1 resolved, 2 rejected; polling again replaces the waker |
| `take<T>` | `(externref f) -> T wire` | the resolved value; consumes the future, once |
| `error<E>` | `(externref f) -> E wire` | the rejection, mapped through `E` as in §15 |
| `drop` | `(externref f)` | drops the future, which **cancels** it |
| `new` | `() -> (externref future, externref resolver)` | a language-created future; JS sees a Promise |
| `resolve<T>` | `(externref resolver, T wire)` | |
| `reject<E>` | `(externref resolver, E wire)` | the JS rejection value is the outgoing mapping of `E` |

### Cancellation (guest only)

Dropping a future cancels it. The JS side of cancellation is explicit call
shape:

```ts
@js(
  let c = AbortController.new(),
  args = (input, init? { signal = c.signal }),
  cancel = c.abort,
)
fetch(input: Request, init: RequestInit?): future<Response, FetchError>;
```

* `let` values live with the future until it settles or is dropped.
* On `drop`, the bridge calls `cancel` once.
  * A failing cancel function is swallowed, and doesn't settle the promise.
  * A later settlement is ignored.
* `$result` names the call's JS result, for APIs that return a cancellable
  handle (`cancel = $result.cancel`).
* JS-side cancellation of language-created futures is out of scope.

Example:

1. Rust awaits `fetch(req)`. The bridge creates `c`, calls
   `fetch(req, { signal: c.signal })`, and returns the Promise as an
   `externref`.
2. The language polls it with its waker.
3. The language drops the future: `("tw:future", "drop")` runs `c.abort()`.
4. The resulting rejection is ignored.

## 17. Exports

`export` marks what the language module exports to JS. Exports follow the
nested-namespace rule:

```ts
export namespace default {
  fetch(request: Request, env: Env, ctx: ExecutionContext): future<Response>;
}
export version(): str;
```

* Every member of an exported namespace is an export. JS sees
  `export default { fetch }`.
* A top-level function can be exported on its own. A top-level getter or
  setter can't be: an ES module binding can't be an accessor, so it's a
  validation error, and an exported namespace carries it instead.
* An export's Wasm export name is its key, as for imports:
  `default.fetch(Request,Env,ExecutionContext)`, `version()`.
* Exports are **incoming**: `@js(args)` reads as a pattern, and dispatch
  applies (§7).
* `export { A as default };` (a list, inside a `module` block) is an
  imported module's export list. The syntax disambiguates the two.
* The export boundary is the top-level exports. Callbacks aren't entry
  points, but they share the entry points' error membrane.

### `export class`

```ts
export class Counter extends DurableObject {
  new(ctx: DurableObjectState, env: Env);
  increment(by: u32): future<u32>;
  get count(): u32;
  static create(): Counter;
}
```

* An instance is the language's **handle** ([§14](#14-handles)), and it can
  appear in any type position.
* An exported class may extend an imported class, or nothing. In the IR an
  imported class is only its members, so `extends` means the prototype
  chain plus the inherited imported members applying to the instance (a free
  upcast).
* The bridge realizes it as a real JS class:

  ```js
  const handles = new WeakMap();
  class Counter extends DurableObject {
    constructor(ctx, env) {
      super(ctx, env);
      handles.set(this, lang["Counter.new(DurableObjectState,Env)"](this, ctx, env));
    }
    increment(by) {
      return lang["Counter#increment(u32)"](this, handles.get(this), by);
    }
  }
  ```

  The handle lives in the bridge's pairing registry, a `WeakMap`, which
  function wrappers share: a JS function can't carry a class's private
  fields.

* **Every instance export, including `new` (called after `super`),
  receives `(this: externref, handle, args…)`.**
  * `this` is borrowed for the call. An async method holds it until its
    future settles.
  * In linear, the language must not store `this`. It would sit in the
    externref table (a GC root), and the instance would never be finalized.
  * GC languages may cache `this`, since that cycle lives in one heap.
  * `new` returns the handle.
* `super` arguments pass through by default. `@js(super = (...))` overrides
  them.
* Statics become statics on the JS class, called with no receiver.
* Finalizers (linear, profile option): `tw:drop:Counter(h)`.
* Custom descriptors (WebAssembly/custom-descriptors) are out of scope.

* **Instance lifetime follows whether the wrapper can be recreated.** A
  wrapper that carries no state of its own can be recreated unobservably:
  once JS has dropped it, nobody holds the old one to compare against.
* **A base-less class** is constructed natively by the language. The bridge
  makes its wrapper when the handle first crosses to JS, with
  `Object.create(Counter.prototype)` (the JS constructor doesn't run), and
  again if the wrapper was collected while the language kept the instance.
  An instance that never crosses has no wrapper.
  * In linear, each wrapper's finalizer drops JS's hold once
    (`tw:drop:Counter`, §14), and a re-made wrapper takes it again.
  * A JS subclass instance (`class MyCounter extends Counter` in JS) is only
    ever a `Counter` to the language. After its wrapper is collected it
    comes back as a plain `Counter`, so subclass state isn't preserved.
* **A class with an imported base** is constructed through JS, since only a
  real `new` gives the base its internals, so its wrapper can't be
  recreated. The language constructs one (in a static factory such as
  `create()`, or anywhere else) with the import `("tw:new", ctor)`, named by
  the constructor's key: `("tw:new", "Counter.new(DurableObjectState,Env)")`.
  Its signature is the constructor's read as an import,
  `(W(args)) -> externref`.
  1. The bridge runs `new Counter(...)` with the arguments as JS passes
     them, so `super` gets its real arguments (`@js(super = (...))` if
     given), and dispatch takes the named constructor even where an earlier
     overload would match.
  2. The JS constructor calls the language's `Counter.new(...)` export,
     which returns the handle, and pairs it with the new wrapper.
  3. `tw:new` returns the wrapper. The language recovers the handle with
     `("tw:unbox", C)` or `("tw:tryUnbox", C)`.

  Construction re-enters the language, as callbacks do, so the export runs
  inside the `tw:new` call. `tw:new` takes only such a constructor.
  * Returning a handle with no live wrapper is a `TypeError`: one the
    language built without `tw:new`, one destroyed, or in linear one whose
    wrapper was collected (and dropped).
  * In linear, a language that keeps an instance past JS's interest pins its
    wrapper: it takes it with `("tw:box", C)`, keeps it (in the externref
    table, say), and releases it when the language drops the instance.
  * In GC, pairing is a `WeakMap` keyed by the handle, so the wrapper lives
    as long as either side holds it, and nothing is pinned.

## 18. Callbacks

Function types are first class and structural: `fn(Event)`,
`fn(T, u32) -> U`, `fn(Event)?`, `arr<fn(Event)>`,
`fn(Event) -> unit throws E`. Identical types share one set of operations.

| direction | value | operations |
|---|---|---|
| language → JS (the language provides the function) | a handle | export `tw:call:F(env, args…) -> R` (the trampoline); linear: export `tw:drop:F(env)`; import `("tw:destroy", F)`; into an erased position, `("tw:box", F)` |
| JS → language (JS provides the function) | `externref` | import `("tw:call", F)(fn, args…) -> R` |

* The direction is the position's: an import parameter is
  language → JS; an import result or an export parameter is JS → language.
* The trampoline converts its arguments outgoing to the language (incoming
  from JS) and its result back to JS. The call import does the reverse.
* `throws E` on a function type follows the import rules for the call import
  and the entry-point rules for the trampoline: **an error escaping a
  language callback into JS is mapped at the trampoline exactly as at entry
  points.**
* Synchronous reentrancy (`forEach`, a `Promise` executor) is ordinary
  nested reentrancy, and it's allowed.
* A **rest parameter** `fn(A, ...arr<T>)` (`arr<T>` or `Arr<T>`, last) is
  part of the type, so `fn(...arr<f64>)` and `fn(arr<f64>)` are distinct types with
  distinct keys. The wire is the sequence's. When JS calls a language
  function, the remaining arguments bind into the sequence (none binds an
  empty one), as `...rest` does incoming (§7).
  `("tw:call", F)` spreads it as the remaining arguments.
* The JS receiver (`this`) of a callback call is ignored. Binding it to a
  parameter is future work (§22).
* Keys: mono keys are concrete at each use (`fn(f64,u32)->Str`). Erased type
  parameters become boxes, so the key is the erased shape.
* Pairing, finalizers, `destroy` and `@once` are as in [§14](#14-handles).

* A JS → language `fn` position stays `externref`, since it can hold any JS
  function. When the value is a wrapper of the language's own handle (an
  `onclick` the language set, read back), the language recovers the handle
  through the pairing with `("tw:unbox", F)` ([§13](#13-boxes-reference-wire-types-and-generics)).
  Calling it through `("tw:call", F)` also works.

Example:

```ts
class EventTarget {
  addEventListener(type: str, listener: fn(Event));
}
```

* Rust (linear): the import has the wire `(externref, i32, i32, i32)`: the
  target, the `str`, and the env handle.
* When JS dispatches the event, the bridge calls
  `tw:call:fn(Event)(env, event)`.
* If the callback throws `tag:err`, JS sees the mapped error at the dispatch
  site.

## 19. The `tw` protocol

Every bridge ↔ language protocol member is a (group, member) pair in the
`tw` protocol. User keys never start with `tw:`, so the two can't collide.

* **Imports** (the bridge provides): module `tw:<group>`, name = the member
  (for typed operations, the subject type or path in canonical spelling).
* **Exports** (the language provides): one name. Typed operations join the
  op and the subject with `:` (`tw:drop:Counter`). Grouped operations join
  with `/` (`tw:loop/tick`). Allocation and the termination flag are
  ungrouped.
* Every default name is configurable, so fixed toolchain names (such as
  wasm-bindgen's `__instance_terminated`) can be mapped.

Notation in the tables:

* `ptr`/`len`/`size`/`align` are `i32` (memory32) or `i64` (memory64).
* `handle` is `i32`/`i64` (linear) or `anyref` (GC).
* `W(T)` is `T`'s wire ([§10](#10-primitives-and-validation),
  [§11](#11-sequences-and-memory)).

### Bridge imports

| module | name | signature | notes |
|---|---|---|---|
| `tw:js/global` | user key | per key | ambient globals (§5) |
| module spec | user key | per key | module declarations (§5) |
| `tw:instanceof` | class path | `(externref) -> i32` | classes only; 0 for a class with no value (§8) |
| `tw:destroy` | `F` or class path | `(handle)` | §14 |
| `tw:call` | `F` | `(externref fn, W(args)…) -> W(R)` | call a JS function |
| `tw:box` | `T` | `(W(T)) -> externref` | §13 |
| `tw:unbox` | `T` | `(externref) -> W(T)` | checked |
| `tw:unboxUnchecked` | `T` | `(externref) -> W(T)` | unchecked |
| `tw:tryUnbox` | `C` or `F` | `(externref) -> W(C?)` | the handle or none (§13) |
| `tw:new` | constructor key | `(W(args)) -> externref` | constructs an exported class with an imported base through JS (§17) |
| `tw:arr` | `len` | `(externref) -> i32` | §13 |
| `tw:arr` | `get<T>` | `(externref, i32) -> W(T)` | §13 |
| `tw:isNullish` | empty | `(externref) -> i32` | 1 for `undefined` or `null` (§13) |
| `tw:isUndefined` | empty | `(externref) -> i32` | 1 for `undefined` (§13) |
| `tw:unit` | empty | `() -> externref` | the unit box, `undefined`: a box's none (§13); the table's `const_base` |
| `tw:true`, `tw:false` | empty | `() -> externref` | the `Bool` boxes (§13); the table's `const_base + 2`, `+ 3` |
| `tw:tag` | enum path | tag `(W(E))` | §15; `err` for the built-in |
| `tw:future` | `poll` | `(externref, i32 waker) -> i32` | §16; 0 pending, 1 resolved, 2 rejected |
| `tw:future` | `take<T>` | `(externref) -> W(T)` | |
| `tw:future` | `error<E>` | `(externref) -> W(E)` | |
| `tw:future` | `drop` | `(externref)` | cancels |
| `tw:future` | `new` | `() -> (externref, externref)` | future, resolver |
| `tw:future` | `resolve<T>` | `(externref, W(T))` | |
| `tw:future` | `reject<E>` | `(externref, W(E))` | |
| `tw:loop` | `work_available` | `(i32 loop)` | |
| `tw:loop` | `drop` | `(i32 loop)` | |
| `tw:ret` | `<vt>` | `() -> vt` | `results = single`: pop the next spilled slot |
| `tw:ret` | `push:<vt>` | `(vt)` | `results = single`: push the next slot |
| `tw:err` | `E` | `(externref exn) -> W(E)` | `throws = js` |
| `tw:err` | `set:E` | `(W(E))` | `throws = js` |

### Language exports

| name | signature | when |
|---|---|---|
| `tw:realloc` | `(ptr old, size old, align, size new) -> ptr` | linear memory configured |
| `tw:free` | `(ptr, size, align)` | linear memory configured |
| `tw:table/alloc` | `(i32 n) -> i32 base` | externref table used |
| `tw:table/free` | `(i32 base, i32 n)` | externref table used |
| `memory` | memory | linear memory configured |
| `tw:table` | table `externref` | externref table used |
| `tw:table/stack_base`, `tw:table/const_base`, `tw:table/heap_base` | global: immutable `i32` | `tw:table` exported (§14) |
| `tw:terminated` | global: immutable `ptr` address (linear memory) or mutable `i32` state (GC, no memory) | termination flag option |
| `tw:loop/tick` | `(i32 loop, i32 max) -> i32` | async used |
| `tw:loop/next_sleep` | `(i32 loop) -> W(u53?)` | async used |
| `tw:future/wake` | `(i32 waker)` | async used |
| `tw:call:F` | `(handle env, W(args)…) -> W(R)` | per callback type the language provides |
| `tw:drop:F` | `(handle env)` | linear only |
| `tw:drop:C` | `(handle)` | linear, exported class, finalizer option |
| user export key | per key; instance members take `(externref this, handle, …)` | §17 |

## 20. End-to-end example

```ts
ir 1;

enum FetchError extends err {
  @js(DOMException[name = "AbortError"]) Aborted(Str);
}

class Request {
  get url(): str;
}
class Response {
  new(body: str);
}

@js(
  let c = AbortController.new(),
  args = (input, init? { signal = c.signal }),
  cancel = c.abort,
)
fetch(input: Request, init: RequestInit?): future<Response, FetchError>;

export namespace default {
  fetch(request: Request, env: Env, ctx: ExecutionContext): future<Response>;
}
```

Language imports (both profiles, except where noted):

| import | wire |
|---|---|
| `("tw:js/global", "Request#get:url()")` | `(externref) -> (i32, i32)` (linear) / `(ref $str)` (GC, `gc` str) |
| `("tw:js/global", "Response.new(str)")` | `(i32, i32) -> externref` (linear) / `((ref $str)) -> externref` (GC) |
| `("tw:js/global", "fetch(Request,RequestInit?)")` | `(externref, externref) -> externref` |
| `("tw:future", "take<Response>")` | `(externref) -> externref` |
| `("tw:future", "error<FetchError>")` | `(externref) -> (i32, externref)` |

Language exports: `default.fetch(Request,Env,ExecutionContext)`, of type
`(externref, externref, externref) -> externref` (a future made with
`("tw:future", "new")`), plus `tw:realloc`/`tw:free` (linear) and the loop
exports.

At runtime:

1. JS calls the module's `default.fetch(req, env, ctx)`. The bridge checks
   the termination flag (if enabled) and calls the export.
2. The language reads `req.url` (copied into its carrier), calls `fetch`,
   and polls the returned future.
3. If the request is aborted, `error<FetchError>` yields
   `(1 or 2, message)`: `Aborted` when the rejection is a
   `DOMException` named `AbortError`, `TypeError` for a `TypeError`, and
   otherwise `(-1, message)`.
4. The language resolves its own future with `Response.new(...)`, and JS's
   Promise resolves with that `Response`.
5. If the language rejects with an `err` value, JS receives
   `new TypeError(message)` or `new Error(message)`.

Arm indices for `FetchError`: `TypeError = 1`, `Aborted = 2`, `Error = -1`.

## 21. Grammar

```
file        = "ir" "1" ";" ("lib" ident ";")? ("with" ident ";")* item*
item        = doc* attr* decl
attr        = "@js" "(" jsentry ("," jsentry)* ","? ")" | "@fallback" | "@once"
decl        = namespace | class | interface | enum | module | member
namespace   = "export"? "namespace" name "{" item* "}"
class       = "export"? "class" name tparams? extends? "{" (doc* attr* (assoc | member))* "}"
interface   = "interface" name tparams? extends? "{" (doc* attr* (assoc | member))* "}"
assoc       = "type" name "=" "match" ident "{" row ("," row)* ","? "}" ";"
row         = type "=>" type
extends     = "extends" typeref ("," typeref)*
enum        = "enum" name (":" inttype | "extends" path ("," path)*)? "{" (arm | doc* attr* "static" member)* "}"
arm         = doc* attr* name ("(" type ("," type)* ")")? ("=" (int | "." name))? ";"
module      = "module" string "{" item* exportlist? "}"
exportlist  = "export" "{" name ("as" name)? ("," name ("as" name)?)* "}" ";"
member      = "export"? "static"? ( "get" name "(" ")" ":" type throws?
                                  | "set" name "(" param? ")" throws?
                                  | "new" name? "(" params? ")" throws?
                                  | name tparams? "(" params? ")" (":" type)? throws? ) ";"
throws      = "throws" path?
params      = param ("," param)*
param       = pattr* name ":" "union"? type
pattr       = "@js" "(" ("null" | "unshared") ")" | "@once"   // `@js(null)` is the type's
tparams     = "<" tparam ("," tparam)* ">"
tparam      = ident (":" type | "in" "(" type ("," type)* ")")? ("=" type)?
type        = "@js" "(" "null" ")" type | prefix postfix*  // the attribute needs a `T?`
postfix     = "?" | "[" "]"
prefix      = "*" "mut"? primary "[" "]" | "mut" primary "[" "]" | fntype | primary
fntype      = "fn" "(" ((type ",")* "..."? type)? ")" ("->" type)? throws?
primary     = scalar | "bigint" | "str" | "unit" | "never" | "Any" | box
            | "arr" "<" type ">" | "future" "<" type ("," path)? ">"
            | "(" type ")" | "(" type "," ")" | "(" type ("," type)+ ")"
            | typeref
box         = "Bool" | "I8" | "U8" | "I16" | "U16" | "I32" | "U32" | "I53" | "U53"
            | "I64" | "U64" | "F16" | "F32" | "F64" | "BigInt" | "Str" | "Unit"
            | "Vec" "<" type ">" | "Arr" "<" type ">" | "Tuple" "<" type ("," type)* ">"
scalar      = "bool" | "i8" | "u8" | "i16" | "u16" | "i32" | "u32" | "i53" | "u53"
            | "i64" | "u64" | "f16" | "f32" | "f64"
inttype     = "i8" | "u8" | "i16" | "u16" | "i32" | "u32" | "i64" | "u64"
typeref     = path ("<" type ("," type)* ">")?
path        = ("." | string ".")? name ("." name)*
name        = ident | string
jsentry     = string                                    // enum arm literal
            | classmatch                                // error arm
            | "name" "=" string | "null" | "object" | "index"   // `null`: a member's result's
            | "symbol" "=" ident | "this" "=" ident
            | "new" "=" name ("." name)*
            | "args" "=" "(" tmpl? ")" | "super" "=" "(" tmpl? ")"
            | "let" ident "=" expr | "cancel" "=" expr
classmatch  = classpred ("|" classpred)*
classpred   = path ("[" ident "=" literal ("," ident "=" literal)* "]")?
tmpl        = telem ("," telem)* ","?
telem       = ident | literal | "undefined" | "..." ident
            | ident "?" "{" ident "=" expr ("," ident "=" expr)* "}"
            | expr
expr        = literal | ("$result" | ident | path) ("." name)* ("(" (expr ("," expr)*)? ")")?
literal     = string | int | "true" | "false" | "null"

key         = path ("#" | ".") mname targs? "(" (ctype ("," ctype)*)? ")"
            | (path ".")? ("get:" | "set:") name "(" ctype? ")"   // global values
            | (path ".")? name targs? "(" (ctype ("," ctype)*)? ")" // top-level functions
mname       = name | "get:" name | "set:" name | "new" | "new:" name
targs       = "<" ctype ("," ctype)* ">"
ctype       = the canonical spelling of a type (§3)
```

* `T: C` is a subtype bound and `T in (A, B)` a concrete bound (§13).
* `type` begins an associated type only before a name and `=`, so it stays
  a member name (§13).
* In a `ctype`, a type declared in a `module` block is the path
  `"spec".Name` (§3); `name` already admits the quoted segment.
* A `jsentry` that's a bare path (the error class) and one that's a string
  are told apart by token.

## 22. Future work

Deferred, and not part of this version:

* error `stack` and `cause`; multi-field `throws` arms;
* reinit after termination (it needs a bulk destroy of every handle, plus
  an instance generation so stale finalizers can't reach the new instance);
* multiple memories, and table64;
* tagged unions (`@js(when)`);
* JS-side cancellation of language-created futures;
* JSPI;
* custom descriptors for exported classes;
* language-built GC scalar boxes as an optional fast path (`tw:unbox` would
  accept them);
* symbols beyond the well-known ones;
* binding a JS receiver to a parameter (`@js(this = p)`), for callbacks whose
  `this` carries meaning; the entry is reserved and rejected;
* a general intrinsic namespace beyond the typed-array and `DataView`
  `view` intrinsics;
* `tw:arr` operations beyond `len` and `get`;
* Rust directly on the `tw` ABI (reading 2), pending externref support in
  Rust. Until then Rust bindings are generated through wasm-bindgen from the
  IR. Every IR decision stays compatible with externref-native Rust.

## Open items

None. Every question raised while writing this version is settled in the
sections above, and everything deferred is listed under
[Future work](#22-future-work).
