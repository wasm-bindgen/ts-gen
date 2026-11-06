# The ts-gen IR

The IR is a `.d.ts` expanded into uniquely named bindings with wasm-level
types. ts-gen writes it; language generators (Rust, Zena, Kotlin) read it
to write a language's bindings, and the bridge builder reads it to build
`bindings.wasm`, which implements every binding in JS terms. A generator
needs nothing from TypeScript or ts-gen: overloads are already expanded
into separately named members, naming options are applied, and every
position has its wire type. This document specifies the text form;
`ts_gen_abi::text::{emit, parse}` (model in `abi/src/ir.rs`) are its
reference writer and reader, and `parse(emit(ir)) == ir`.
[Bridge](#bridge) specifies what `bindings.wasm` adds on the wire.

```ts
ir 1;

/// An HTTP request.
class Request extends Body {
  new(input: String);
  new(input: Request, init: RequestInit | Undefined) as new_with_request;
  get url: String;
  text(): Promise<String>;
  waitUntil(promise: Promise<Any>) as wait_until;
  static redirect(url: String, status: f64): Response throws;
}

shape class ResponseInit {
  new {};
  get status: Number | Undefined;
  set status(value: Number | Undefined) as set_status;
}

kind HeadersKind {
  Headers = Headers;
  Record = Record<String>;
  Pairs = Array<Array<String>>;
}

function fetch(input: Request, init: RequestInit | Undefined): Promise<Response>;

module "cloudflare:sockets" {
  function connect(address: String): Socket;
}

provide default.fetch(request: Request, env: Env, ctx: ExecutionContext): async Response;
```

## Lexical structure

* The file is UTF-8 and starts with the header `ir 1;`, the format
  version. A reader rejects a missing header or another version.
* `//` starts a comment to the end of the line, which carries no
  meaning. `///` starts a doc line (`////` is a comment). Consecutive doc
  lines, each with `///` and one following space removed, joined by
  `\n`, are the doc of the next class, kind, member, top-level member or
  provide. A doc before anything else is an error.
* An identifier is a letter, `_` or `$`, then letters, digits, `_` or
  `$` (Unicode letters and digits included). A name that is not an
  identifier is written as a string: `get "content-type": String;`.
* Strings are `"..."` with the escapes `\"`, `\\`, `\n`, `\r`, `\t` and
  `\u{h+}`.
* Keywords are contextual: `ir class shape static extends kind fallback
  function get set new as throws raises nothrow cohort from alias module
  provide async globalThis`. Only `new` as an instance method's name needs quoting
  (`"new"(): X as new_instance;`); the writer quotes it in every member
  name.

## Types

Lowercase types are wasm values. Uppercase types, and literals, are JS
values carried as `externref`.

| Text | Model (`Type`, `Head`) | Value |
| --- | --- | --- |
| `void` | `Void` | no value; results and callback results only |
| `f64` | `F64` | a JS `number` |
| `i64` | `I64` | a JS `bigint` |
| `bool` | `Bool` | a JS `boolean` as `i32`, 0 or 1 |
| `Any` | `Any` | any JS value |
| `String`, `Number`, `Boolean`, `BigInt` | `String`, ... | a JS string, number, boolean, bigint, by reference |
| `Request`, `Promise<T>`, `Array<T>`, `Record<T>` | `Named` | an instance of a class or kind, by binding name, or of an undeclared global type |
| `R` | `Var` | a type parameter of the enclosing class or member |
| `(String, f64) => void` | `Callback` | a JS function taking and returning those |
| `[String, Number]` | `Tuple` | a fixed-length JS array |
| `"GET"`, `0`, `-1` | `Literal` | exactly that string or integer |
| `T \| Null`, `T \| Undefined` | `JsType.null`, `.undefined` | `T`, or `null` / `undefined` |
| `Null`, `Undefined`, `Null \| Undefined` | `Empty` | only the empties: `Promise<Undefined>` is TS `Promise<void>` |

`Null` and `Undefined`, the empties, are union arms beside at most one
other JS type, in any order; the writer puts the type first, then
`Null`, then `Undefined`. Empties alone are the `Empty` type. A wasm
type with an empty is an error: an optional number is `Number |
Undefined`, never `f64 | Undefined`. A union of two non-empty types is
an error too: that is a kind. Type arguments, tuple elements, kind arms
and exception types are JS types. A callback with an empty is
parenthesised, `((Any) => void) | Null`, since a callback's result
extends as far right as a type can: `(A) => String | Undefined` returns
`String | Undefined`.

Type arguments erase on the wire; they tell a generator what a
`Promise`, an `Array` or a generic class holds.

### Wire types

Every JS type is `externref` at the boundary, exactly as JS produced it.

* A JS type admitting neither empty may be taken as `(ref extern)`.
* `null` admits a wasm null: JS `null` arrives as `ref.null extern`, and
  a language passes `null` as one. Test it with `ref.is_null`.
* `undefined` is a non-null reference. Test it with
  `JsValue.is_undefined`; produce it from the `undefined` global or
  `JsValue.undefined` (see [Bridge](#bridge)).
* A `Number` reference unboxes to `f64` with `Number.value`.
* A GC language may take any JS position as `anyref` instead, and as
  `(ref any)` where `(ref extern)` is allowed.

## Declarations

At top level: `class`, `kind`, top-level members, `alias`, `module`
blocks and `provide`. Inside a `module` block: all of them but `module`
and `provide`.

### Classes

```ts
class ReadableStream<R> extends EventTarget, Base<R> { members }
shape class ResponseInit { members }
static class WebAssembly { members }
class WebAssembly.Module {}
class H as H2 {}
```

A class is every JS value type with members. Its path (`Class.path`) is
the dotted JS path from the root (`globalThis`, or the module's
namespace in a `module` block). Its binding name (`Class.name`), after
`as` when it differs from the path, is unique among the module's
classes and kinds: member imports and type references use it, and
`(module, name)` is the class's identity. Two classes may share a path,
as a block's local `H` beside an exported one, `class H as H2`. A
`static class` is never a type and a kind has no imports, so the two
may share a binding name, as a numeric enum `Level` beside a `namespace
Level` of functions. The
modifier says what is at the path at runtime (`Class.kind`):

| Text | `ClassKind` | At the path |
| --- | --- | --- |
| `class` | `Class` | a constructor, which `instanceof` tests against |
| `shape class` | `Interface` | nothing: values only a structural check can tell |
| `static class` | `Namespace` | an object with only static members, which is not a type |

`<R, ...>` are type parameters, in scope in the class's members and
`extends`. `extends` lists the classes this one inherits from, as
written in the `.d.ts`; inherited members are only on the base, and a
generator presents them on the derived class and may pass a derived
instance where a base is expected.

`class` is the only object declaration: there is no `interface`,
`dictionary` or `namespace` keyword.

### Kinds

```ts
/// An HTTP method.
kind Method as MethodKind fallback {
  /// Fetches.
  Get = "GET";
  Post = "POST" alias Create, Submit;
  Other = String;
}
```

A named union. A generator matches on the arms in a result (see
[Recognition](#recognition)), and takes any arm in an argument. Like a
class, a kind has a JS path (where a numeric enum's object is; the name
itself for a union no declaration names) and a binding name after `as`,
which shares the class namespace.
`fallback` makes it open: a value no other arm matches is the last
non-literal arm's, a shape that is not tested for.

Each arm is `<name> = <type> [alias <name>, ...];` with an optional
doc: a variant name unique in the kind, aliases included (later enum
members repeating the value), and a JS type admitting no empties.

The empties belong to positions, not arms: a position that also admits
`null` or `undefined` says so itself, `get method: Method | Undefined`,
where the Rust binding has `Option<Method>`. A Rust enum has no empty
variant, so a kind has no empty arm. A union alias with empties, `type
MaybeDay = Day | string | null | undefined`, is the kind of its other
arms, and a position naming it admits its empties. An arm that holds
any value (`JsValue = Any`, an unresolved name) still admits none: a
non-null position of its kind is non-null.

#### Recognition

Where JS produces a kind's value (an import's result, a provide's
parameter), the value's arm is the one Rust's binding reads it as
(wasm-bindgen's dynamic union, string and numeric enums):

1. The literal arms are tested first, in any order (their values are
   distinct).
2. Then the other arms, in order. In a `fallback` kind the last of them
   is not tested: it holds whatever no earlier arm does.
3. The first arm that holds the value is its arm. A value no arm holds
   has none, which Rust reports as an error, except for a kind of only
   string literals, which it reads as a hidden invalid variant.

A position's empties are its own: a position admitting `null` or
`undefined` tests for them before its kind's arms. An arm holds `x`
when:

| Arm | Test |
| --- | --- |
| `"s"`, `n` | `x === "s"`, `x === n` |
| `String`, `Number`, `Boolean`, `BigInt` | `typeof x` is `"string"`, `"number"`, `"boolean"`, `"bigint"` |
| `Any` | always |
| a `class` `C` | `x instanceof C`, at `C`'s path from its root (the module's namespace or `globalThis`); false if looking it up or testing throws |
| a `shape class` | `typeof x === "object" && x !== null` |
| a kind `K` | `x` has an arm of `K` |
| `Array<T>` | `Array.isArray(x)` and every element holds as an element of `T` (below) |
| a tuple | `Array.isArray(x)`; the elements are not tested |
| `Record<T>` | `x instanceof Object` |
| a callback, `Function` | `typeof x === "function"` |
| `Symbol` | `typeof x === "symbol"` |
| `ArrayBufferView` | `x instanceof Uint8Array`, the type Rust reads it as |
| another undeclared `G` (`Promise<T>`, `ArrayBuffer`) | as a `class` `G` |

An element of `T` holds as an arm of `T` would, except that an element
whose type admits an empty holds when it is that empty, an `Array<U>`
element by `Array.isArray` alone, and a literal, or a kind of only string
(integer) literals, by `typeof` `"string"` (`"number"`) alone: Rust reads
`Array<T>` as a `Vec` of `T`'s element type (`Vec<Array<JsString>>`,
`Vec<String>`).

### Members

The general form is

```
<modifiers> <jsName>(<params>): <Result> <exception> as <name>;
```

`<jsName>` may be followed by `?`, `get x?: T`: a property declared
optional (`x?: T`, `Member.optional`), which a plain-object builder may
leave out, as it may not `x: T | Undefined`. Then `<T, ...>`, the
member's own type parameters (`Member.type_params`), in scope in its
parameters, result and exception besides the class's, which they may
not repeat: `pipeThrough<T>(transform: Any): ReadableStream<T>;`,
`new Q.R<T>(a: T)`. The JS name comes first
because that is the JS surface; `as <name>`
gives the binding name (`Member.name`) when it differs from the default,
which is the JS name, and `new` for the two `new` forms. Names are
unique within a class, aliases included, and top-level names unique
within their module. An omitted result is `void`, except for the `new`
forms, where it is the class itself applied to its own type parameters,
non-null (`ReadableStream<R>`).

In class `P`, with `root` the global object or the module's namespace:

| Text | `MemberKind` | JS operation | Receiver |
| --- | --- | --- | --- |
| `new(a: A)` | `Constructor` | `new P(a)` | none |
| `new Q.R(a: A)` | `Constructor` | `new Q.R(a)` | none |
| `new {}` | `Object` | `{}` | none |
| `get x: T` | `Get` | `this.x` | first |
| `set x(value: T)` | `Set` | `this.x = value` | first |
| `x(a: A): R` | `Method` | `this.x(a)` | first |
| `static get x: T` | `StaticGet` | `P.x` | none |
| `static set x(value: T)` | `StaticSet` | `P.x = value` | none |
| `static x(a: A): R` | `StaticMethod` | `P.x(a)`, with `P` as `this` | none |

Top level, the same on `root`:

| Text | `MemberKind` | JS operation |
| --- | --- | --- |
| `function f(a: A): R` | `StaticMethod` | `root.f(a)`; a module's function is called with `this` undefined |
| `get x: T` | `StaticGet` | `root.x` |
| `set x(value: T)` | `StaticSet` | `root.x = value` |

`P` and `Q.R` are paths from the root. A constructor's JS name
(`Member.js`) is the dotted path it constructs, the class's own unless
written after `new`; `new {}` has the class's path as its JS name and
does not use it. A JS name of the form `[Symbol.iterator]` is that
well-known symbol's key.

The import of an instance member takes the receiver first, an
`externref` of the class that is never null, then the parameters. A
getter may take parameters (`get x(i: f64): T`), written only when
present.

`<exception>` is one of:

| Text | Model | Meaning |
| --- | --- | --- |
| | | the binding does not catch |
| `throws` | `throws` | the binding catches a JS exception and reports it to the caller |
| `throws T` | `throws`, `errors: [T]` | as `throws`, with the declared exception type `T` |
| `raises T` | `errors: [T]` | does not catch; `T` is what a catching form derived from it reports |

`T` may be a union, `throws TypeError | RangeError`, each arm one
exception type (`@throws {TypeError | RangeError}`), so `Null` and
`Undefined` arms stand alone rather than qualify another; the arms are
distinct.

followed by an optional `nothrow`: declared never to throw (`@throws
{never}`), so no catching form derives from the binding. How a caught
exception crosses to the language is the bridge's convention, not this
format's (see [Exceptions](#exceptions)).

Then an optional `cohort <name>` (`Member.cohort`): the member is an arm
of a literal-keyed cohort, members that differ only in the literals they
pass (each a literal-typed parameter), and `<name>` is the binding the
cohort's arms share, which takes those literals as plain values:

```ts
static of(kind: "a"): Counter cohort __of as of_a;
static of(kind: "b"): Counter cohort __of as of_b;
```

Every member is its own import, cohort arms included, with its literal
parameters on the wire like any other: `of_a` takes the string `"a"`.
`cohort` only names the shared binding a generator may present instead,
as Rust does; the bridge never imports it.

Then an optional `from "<spec>"` (`Member.module`): the binding is
imported from module `spec` rather than its class's, as a constructor
another module's variable exports is: `new(addr: String) from
"cloudflare:workers";`.

### Variadics

`log(...data: Array<Any>)`: the last parameter may be a rest parameter,
whose type is `Array<T>`. On the wire it is `(ref null $anyrefs)`, where
the language module declares `(type $anyrefs (array (mut anyref)))`, and
the bridge spreads the array's elements into the call.

### Modules

```ts
module "cloudflare:workers" {
  class DurableObject<Env> { ... }
  function connect(address: String): Socket;
}
```

Declarations in a `module` block belong to the module `"spec"`: their
paths are rooted at its export namespace, and their imports are from
`"spec"`. Several blocks may name one module.

### Aliases

```ts
alias Connection = Conn;           // Class.aliases
alias Conn.open2 = Conn.open;      // Member.aliases of Conn.open: "open2"
alias enqueue = queueMicrotask;    // Member.aliases of a top-level member
```

The right side names, in the alias's module and by binding name, a class first, else
`<class>.<member>` (and the left side is then `<class>.<alias>`), else a
top-level member. An alias may come before or after what it names.

### Provides

```ts
provide default.fetch(request: Request, env: Env, ctx: ExecutionContext): async Response;
provide default.add(a: f64): f64 as "default.add";
```

A function the language module exports to JS, the reverse of a member.
The path (`Provide.js`) is where JS reads it from the module's
namespace: `default.fetch` is `export default { fetch }`. The export
name the language module exports it under defaults to the path's last
segment; `as` overrides it. Parameters arrive owned; the result is
owned. `async T` (`is_async`): the JS function returns a `Promise<T>`;
the language returns the promise, or, under JSPI, `T` itself, with the
bridge wrapping the export in `WebAssembly.promising`.

## Imports

Every member is one import into the language module:

* module: `./bindings.wasm` for a global declaration, the specifier for
  one in a `module` block;
* name: `<class name>.<name>`, the class's binding name, or `<name>`
  for a top-level member.

`(module, name)` is unique. The same function is also imported under
every alias: `<class alias>.<name>` for each class alias,
`<class name>.<member alias>` (and `<class alias>.<member alias>`) for
each member alias, `<alias>` for a top-level alias. A generator that
binds a class once, with its aliases as type aliases, may ignore them.
`Ir::imports` (`abi/src/ir.rs`) lists every member with its names. The
bridge adds a few functions of its own, the [sys
functions](#sys-functions) and a [discriminator](#kind-discriminators)
per kind.

## Bridge

`ts-gen embed` reads a language module and the IR it was written
against, and builds `bindings.wasm`, which implements each member the
module imports in JS terms, and `wrapper.wasm`, which publishes the
module's provides. Merged with the module, they leave it importing only
`./builtins.js`, the string constants and the IR's module specifiers.

### Sys functions

Every `bindings.wasm` also has three functions the IR does not list,
fixed by this spec (`ts_gen_abi::ir::Sys`):

| Import | Signature | JS |
| --- | --- | --- |
| `"./bindings.wasm" "JsValue.is_undefined"` | `(param externref) (result i32)` | `x === undefined` |
| `"./bindings.wasm" "JsValue.undefined"` | `(result externref)` | `undefined` |
| `"./bindings.wasm" "Number.value"` | `(param externref) (result f64)` | `Number(x)` |

They win over a member of the same import name. `Number.value` is JS
coercion, not a test: `Number(null)` is `0`, `Number(undefined)` `NaN`.

### Kind discriminators

`bindings.wasm` also has a discriminator per kind (`Kind::discriminator`):

| Import | Signature | Result |
| --- | --- | --- |
| `"./bindings.wasm" "kind:<name>"` | `(param externref) (result i32)` | the index in the kind's arms of the arm holding the value, by [Recognition](#recognition); `-1` (`NO_ARM`) for none |

`<name>` is the kind's binding name. A kind of a `module` block is
imported from its specifier, like its module's members, and under
`--bindings-module` as `<spec>:kind:<name>`. A class member's import
name is `<class>.<member>`, so a kind's discriminator and a `static
class` of its name (`kind:Level`, `Level.name`) never meet; like the sys
functions, a discriminator wins over a top-level member of its import
name. The parameter is the value as JS produced it, `externref` or
`anyref`, nullable or not; the discriminator tests `null` and
`undefined` like any value (an `Any` or `fallback` arm holds them), so a
generator tests the position's empties first. Like every binding, a
discriminator is in `bindings.wasm` only when the module imports it.

### Builtins

A module may import two names from `./builtins.js`, and nothing else:
the global `"undefined"`, an immutable `externref` or `anyref` global,
and `"await"`, `(func (param externref) (result externref))` with each
position `externref` or `anyref`, which suspends on a promise and returns
its value (`WebAssembly.Suspending`). A language that cannot import a
global uses `JsValue.undefined`.

### Signatures

An import's signature is the member's: the receiver, the parameters'
wire types and the result's, `void` none. Arity and scalars are exact. A
reference position may be `externref` or `anyref`, and `(ref extern)` or
`(ref any)` unless JS produces the value there (an import result, a
provide parameter, the `await` result) and its type admits `Null`; a
non-null result JS returns as `null` traps. A rest parameter is `(ref
null $anyrefs)`, exactly `(array (mut anyref))`. A `bool` argument
reaches JS as a boolean, and so does a provide's `bool` result.

### String constants

The bindings take property keys as imported string constants, from
`wasm:js/string-constants` or the namespace `embed
--imported-string-constants` names: the `importedStringConstants` the
merged module is compiled with (Kotlin's is `'`).

### Exceptions

The bridge does not catch. A JS exception a binding's JS throws
propagates through the bindings into the language module as a Wasm
exception; `throws`, `raises` and `nothrow` tell a generator where to
catch it and what it may be. How a catching binding reports a caught
exception to its caller is the generator's choice, not yet fixed on the
wire. `embed` accepts modules that use the legacy exception-handling
instructions (`try`/`catch`), as Kotlin/Wasm emits.

### Provides and JSPI

A provide is an export of the language module under its export name,
with its parameters' wire types and its result's; an `async` provide
always has a result, `externref` for `async void`. The wrapper
publishes it at its path: each first segment is an export of the merged
module holding the function or an object of them. Importing `await`
opts the module into JSPI: the wrapper wraps each provide in
`WebAssembly.promising`, so the export returns `T` and JS receives a
promise of it. Without the import, an `async` provide returns the
promise itself. Importing `await` with no provides is an error.

### Bindings module

A language that cannot choose its import modules, as Kotlin's `@JsFun`
imports only from `js_code`, imports every binding from one module,
`embed --bindings-module <name>`: a global binding under its import
name, a module's under `<spec>:<name>` (`cloudflare:sockets:connect`,
`node:fs:constants.o_rdonly`). The bridge resolves such a name against
the IR's specifiers, a global name first. The module's other imports
from it are left to the host.

## Name resolution

A type name is a dotted binding name. Unqualified, in a class in module
`M`, it is, in order:

1. a wasm type, or `Any`, `String`, `Number`, `Boolean`, `BigInt`,
   `Null`, `Undefined`;
2. a type parameter of the member or the class;
3. a class or kind of `M`;
4. a global class or kind;
5. an undeclared global type, if it starts with an uppercase letter
   (`Promise`, `Map`, an enum the IR does not model);
6. otherwise an error.

`globalThis.P` names the global `P` whatever shadows it, and `"spec".P`
module `spec`'s `P`; neither needs `P` to be declared. The writer emits
the unqualified form when it resolves to the intended identity, else the
qualified one.

## Order

The model's lists are ordered, and the file keeps that order: a reader
appends each class, kind, top-level member and provide to its list in
the order it appears, so declarations of different sorts may interleave
freely. The writer is deterministic: all classes, then all kinds, then
all top-level members, each in model order, with consecutive
declarations of one module grouped into one `module` block, then the
provides. Members keep model order within a class, arms within a kind.
A class's aliases follow the class, class aliases first, then member
aliases in member order; a top-level member's aliases follow it.
Declarations are separated by a blank line, except consecutive
top-level members without docs.

## Grammar

```ebnf
file        = "ir" NAT ";" { decl } ;
decl        = mdecl | module | provide ;
mdecl       = { DOC } ( class | kind | toplevel ) | alias ;
module      = "module" STRING "{" { mdecl } "}" ;

class       = [ "shape" | "static" ] "class" path [ tparams ] [ "as" path ]
              [ "extends" ref { "," ref } ] "{" { { DOC } member } "}" ;
tparams     = "<" IDENT { "," IDENT } ">" ;
kind        = "kind" path [ "as" path ] [ "fallback" ] "{" { { DOC } variant } "}" ;
variant     = name "=" type [ "alias" name { "," name } ] ";" ;  (* no Null, Undefined *)

member      = ( "new" [ path ] [ tparams ] params [ ":" type ]
              | "new" "{" "}" [ ":" type ]
              | [ "static" ] "get" sig [ params ] ":" type
              | [ "static" ] "set" sig params [ ":" type ]
              | [ "static" ] sig params [ ":" type ] ) tail ;
toplevel    = ( "function" sig params [ ":" type ]
              | "get" sig [ params ] ":" type
              | "set" sig params [ ":" type ] ) tail ;
sig         = name [ "?" ] [ tparams ] ;
tail        = [ "throws" [ errors ] | "raises" errors ] [ "nothrow" ]
              [ "cohort" name ] [ "from" STRING ] [ "as" name ] ";" ;
errors      = arm { "|" arm } ;                  (* each arm one JS type *)
params      = "(" [ param { "," param } ] ")" ;
param       = [ "..." ] name ":" type ;            (* "..." on the last only *)

alias       = "alias" segments "=" segments ";" ;
provide     = { DOC } "provide" segments params [ ":" [ "async" ] type ] [ "as" name ] ";" ;

type        = arm { "|" arm } ;                    (* at most one arm besides Null, Undefined *)
arm         = "(" [ type { "," type } ] ")" "=>" type
            | "(" type ")"
            | "[" [ type { "," type } ] "]"
            | STRING | [ "-" ] NAT
            | ref ;
ref         = [ "globalThis" "." | STRING "." ] path [ "<" type { "," type } ">" ] ;
path        = IDENT { "." IDENT } ;
segments    = name { "." name } ;
name        = IDENT | STRING ;
```

`DOC` is a `///` line, `NAT` a decimal integer. Where a rule allows both
an identifier and a keyword, the keyword reading applies only when the
next token fits it: `get(...)` is a method named `get`, `static(...)` a
method named `static`, `new` followed by `(`, `{` or a path is a
constructor, `async` in a provide result is the modifier only when a
type follows, and a `STRING` in a type is a module qualifier only when
`.` follows it, a string literal otherwise.
