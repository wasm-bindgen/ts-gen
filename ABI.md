# The Wasm ABI template format

`ts-gen --target wat` writes the Wasm ABI of a `.d.ts` as one file: a
WebAssembly text module whose imports are the ABI functions, annotated
with everything a bindings generator needs to present them in a
language. This document is the contract between ts-gen, which writes
the file, `ts-gen embed`, which reads it back to implement the ABI,
and bindings generators, which read it to write a language's bindings
without knowing anything about TypeScript or ts-gen.

```sh
ts-gen --input worker.d.ts --target wat --provides ExportedHandler.fetch -o worker.abi.wat
<generator> worker.abi.wat            # writes the language's bindings
<compiler>                            # builds the language module
ts-gen embed --abi worker.abi.wat -m module.wasm -o out/
```

The file is a valid module. It assembles with any WAT parser, since the
[annotations proposal](https://github.com/WebAssembly/annotations)
makes a tool ignore annotations it does not know, so a language that
writes WAT by hand can import from it directly. A generator only needs
an S-expression reader: the template uses a small, fixed subset of the
text format, listed under [Grammar](#grammar).

## Model

The ABI is three lists.

* **Owners**: the classes, interfaces, dictionaries and namespaces whose
  members the ABI binds, with their heritage.
* **Functions**: one import per JS member, each with the JS operation
  that implements it, a Wasm signature, and per reference position what
  the reference is and whether it may be `null` or `undefined`.
* **Handlers**: the functions the language module provides to JS, as
  stub exports.

Every JS value crosses as `externref`, exactly as JS produced it.
`number` is `f64`, `boolean` is `i32` and `bigint` is `i64`, except
where a scalar position also admits `null` or `undefined`, where it is a
reference. CONVENTIONS.md, "Wasm ABI", describes how a `.d.ts` lowers
to this model; this document describes the file.

## Annotations

All annotations are named `@abi.<name>`. Their placement is part of
their meaning, as below. A consumer ignores annotations under other
names, and should ignore `@abi.*` fields it does not know, so later
versions can add information. The operations are a closed set per
version: a new one changes the version, so an import carrying none of
the operations of its version is malformed, and a consumer rejects it.

### Version

```wat
(module
  (@abi.version 1)
  ...)
```

The first field of the module. The version changes when a consumer of
the previous version would misread the file, as with a new operation;
other additions do not change it.

### Owners

```wat
(@abi.class "Request" (extends "Body") (doc "An HTTP request."))
(@abi.interface "Console")
(@abi.dictionary "ResponseInit")
(@abi.namespace "WebAssembly")
(@abi.class "Conn" (module "db") (alias "Connection"))
```

A module field, written before the owner's functions. The string is
the owner's dotted JS path (`WebAssembly.Module`); with `(module
"<spec>")` the owner is declared in `declare module "<spec>"`, and
`(module, path)` is its identity, so `"db"`'s `Conn` and a global `Conn`
are two owners. Everywhere else an owner is named, by `extends` or a
reference head, it is named by that identity: `"<path>"` for a global
owner and `"<path>" (module "<spec>")` for a module's, whatever module
the naming import is in.

| Kind | Meaning |
| --- | --- |
| `class` | A JS class: a constructor object exists at the path, so `instanceof` works |
| `interface` | A TS interface with methods: a shape, with no JS object at the path |
| `dictionary` | A property-only interface: a plain object, with a `Type.new` that creates `{}` |
| `namespace` | A JS object at the path whose members are all static |

Fields:

* `(extends "<path>" (module "<spec>")?)`, repeated: the owners this
  one inherits from, as written in the `.d.ts`. Inherited members are
  not repeated on the derived owner. A generator presents `request.text()` by following
  `Request`'s `extends` to `Body.text`, and may pass any derived owner
  where a base is expected.
* `(alias "<name>")`, repeated: other names the owner is exported under.
  Each of its functions is also imported under each alias (see
  [Aliases](#aliases)).
* `(doc "<text>")`: the declaration's JSDoc.

### Functions

```wat
(import "./bindings.wasm" "Headers.get"
  (func $Headers.get
    (@abi.call "get") (@abi.owner "Headers")
    (@abi.ts "get(name: string): string | null")
    (@abi.doc "Returns the first value of the header.")
    (param $this externref (@abi.ref "Headers"))
    (param $name externref (@abi.ref string))
    (result externref (@abi.ref string null))))
```

An import. Its module is `./bindings.wasm` for a global declaration,
or the specifier of the `declare module` it is declared in. Its name is
`<owner>.<member>`, or the bare member name for a free function or
variable; `(module, name)` is unique.

The member part is a valid identifier in most languages: ASCII,
snake_case, with wasm-bindgen's `_with_<x>` suffixes for overloads. A
generator may use it as is, or derive its own name from the operation's
JS name, which is the name as written in the `.d.ts`.

Annotations inside `func`, before the parameters:

* **Operation**, exactly one: what `embed` does to implement the
  function. See [Operations](#operations).
* `(@abi.owner "<path>")`: the owner this is a member of, in the
  import's module. Absent for a free function, a global variable and
  the sys functions.
* `(@abi.ts "<text>")`: the TS declaration, for documentation.
* `(@abi.doc "<text>")`: the member's JSDoc.

The parameters of an instance operation start with the receiver,
`$this`. Parameter names are sanitised, unique within the function, and
never `this` for a parameter other than the receiver.

#### Operations

| Annotation | JS | Receiver |
| --- | --- | --- |
| `(@abi.get "x")` | `this.x` | the first parameter |
| `(@abi.set "x")` | `this.x = value` | the first parameter |
| `(@abi.call "x")` | `this.x(...args)` | the first parameter |
| `(@abi.get "x" (on "A" "B"))` | `A.B.x` | the object at the path |
| `(@abi.set "x" (on "A" "B"))` | `A.B.x = value` | the object at the path |
| `(@abi.call "x" (on "A" "B"))` | `A.B.x(...args)`, with `A.B` as `this` | the object at the path |
| `(@abi.new "A" "B")` | `new A.B(...args)` | none |
| `(@abi.object)` | `{}` | none |
| `(@abi.is-undefined)` | `x === undefined` | none |
| `(@abi.number)` | `Number(x)` | none |
| `(@abi.undefined)` | `undefined` | none |

A path is rooted at `globalThis` for an import from `./bindings.wasm`,
and at the module's export namespace for an import from a specifier:
`(@abi.call "connect" (on))` imported from `"cloudflare:sockets"` calls
that module's `connect` export, with `this` undefined. `(on)` with no
segments is the root itself, so a global function is `(@abi.call
"fetch" (on))` and `declare var console` is `(@abi.get "console" (on))`.

Several imports with the same owner and operation are overloads of one
JS member: `Headers.new` and `Headers.new_with_init` are both `(@abi.new
"Headers")`. A language with overloading may present them as one name.

#### Sys functions

Every template has the same three functions, with no owner, answering
what only JS can:

| Import | Operation | Signature |
| --- | --- | --- |
| `"./bindings.wasm" "JsValue.is_undefined"` | `(@abi.is-undefined)` | `(param externref) (result i32)` |
| `"./bindings.wasm" "JsValue.undefined"` | `(@abi.undefined)` | `(result externref)` |
| `"./bindings.wasm" "Number.value"` | `(@abi.number)` | `(param externref) (result f64)` |

`null` needs nothing: it is `ref.null extern`, tested with
`ref.is_null`.

#### Aliases

```wat
(import "db" "Connection.get" (func $db:Connection.get (@abi.alias "Conn.get")
  (param $this externref) (result externref)))
```

The same function under another of its owner's names. The import has
the primary's signature, and `(@abi.alias "<primary name>")` names the
primary import in the same module; it carries no other annotations.
A generator that binds the owner once, with the aliases as type
aliases, skips these imports.

### Reference positions

```wat
(param $body externref (@abi.ref null))
(param $init externref (@abi.ref "ResponseInit" undefined))
(result externref (@abi.ref "Response"))
(result externref (@abi.ref number null))
```

Inside each `externref` parameter or result, `(@abi.ref <head>?
<empty>*)` says what the reference is.

* **Head**, at most one: what the position names once `null` and
  `undefined` are set aside, through type aliases.
  * `"<path>" (module "<spec>")?`: a declared type, by its identity as
    an owner. A reference inside `declare module "db"` to a global
    `Conn` is `"Conn"`, and to `"db"`'s own is `"Conn" (module "db")`.
    Every type the input declares has an owner record, but a head may
    also name one the input only references, such as `Promise` from
    TypeScript's lib, or a namespace. A generator presents a head with
    no class, interface or dictionary record as it would no head;
  * `string`: a JS string;
  * `number`, `boolean`, `bigint`: a scalar that crosses as a reference
    because the position admits an empty value; `Number.value` reads a
    `number`.

  No head means anything else: a union of different types, a type
  parameter, a function type, an array, `any`, `object`.
* **Empties**: `null` if the TS type admits `null`, `undefined` if it
  admits `undefined`. A position with neither may be taken as `(ref
  extern)`; one that admits `null` may not, since a JS `null` arrives as
  a Wasm null. `undefined` is a non-null reference.

A reference position without `@abi.ref` has no head and admits neither.

### Variadics

A variadic function's last parameter is `(ref null $anyrefs)`, where the
module declares `(type $anyrefs (array (mut anyref)))`. The bindings
spread the array into the call.

### Handlers

```wat
(func (export "fetch")
  (@abi.provide "default" "fetch") (@abi.promise)
  (@abi.ts "fetch(request: Request, env: Env, ctx: ExecutionContext): Promise<Response>")
  (param $request externref (@abi.ref "Request"))
  (param $env externref (@abi.ref "Env"))
  (param $ctx externref (@abi.ref "ExecutionContext"))
  (result externref (@abi.ref "Response"))
  unreachable)
```

A function the module provides, as a stub export to replace. The export
name is the one the language module exports it under.

* `(@abi.provide "<segment>"...)`: the JS path `embed` publishes it at,
  from the module namespace (`default.fetch` is `export default { fetch
  }`).
* `(@abi.promise)`: the TS result is a `Promise<T>`, and the result is
  `T`'s. Under JSPI the module returns `T` and `embed` wraps the handler
  with `WebAssembly.promising`; otherwise the module returns the promise.
* `(@abi.ts ...)`, `(@abi.doc ...)` as for functions.

### Builtins

Two imports from `./builtins.js` close every template. They are not ABI
functions, carry no annotations, and are the same in every file:

```wat
(import "./builtins.js" "undefined" (global $undefined externref))
(import "./builtins.js" "await" (func $await (param externref) (result externref)))
```

Importing `await` opts the module into JSPI. A language that cannot
import globals uses `JsValue.undefined` instead of the global.

## Grammar

The template uses only these forms of the text format, so a reader need
not handle the rest:

```
file      ::= '(' 'module' '(' '@abi.version' nat ')' field* ')'
field     ::= owner | import | handler | type | builtin
owner     ::= '(' ('@abi.class' | '@abi.interface' | '@abi.dictionary' | '@abi.namespace')
                  string ownerfield* ')'
ownerfield::= '(' 'module' string ')' | '(' 'alias' string ')'
            | '(' 'extends' ownerref ')' | '(' 'doc' string ')'
import    ::= '(' 'import' string string '(' 'func' id annot* param* result? ')' ')'
handler   ::= '(' 'func' '(' 'export' string ')' annot* param* result? 'unreachable' ')'
type      ::= '(' 'type' '$anyrefs' '(' 'array' '(' 'mut' 'anyref' ')' ')' ')'
builtin   ::= '(' 'import' '"./builtins.js"' string '(' ('global' | 'func') ... ')' ')'
param     ::= '(' 'param' id valtype ref? ')'
result    ::= '(' 'result' valtype ref? ')'
valtype   ::= 'i32' | 'i64' | 'f64' | 'externref' | '(' 'ref' 'null' '$anyrefs' ')'
ref       ::= '(' '@abi.ref' head? ('null' | 'undefined')* ')'
head      ::= ownerref | 'string' | 'number' | 'boolean' | 'bigint'
ownerref  ::= string ('(' 'module' string ')')?
```

`annot` is any `(@abi.<name> ...)` list. Comments (`;; ...` to end of
line) may appear anywhere and carry no meaning. Strings use the text
format's escapes: `\"`, `\\`, `\n`, `\t`, `\r`, `\'`, two-digit hex
`\hh` for a byte, and `\u{h+}` for a code point.

## Consumers

* **`ts-gen embed --abi <file>`** reads the file in place of the
  `.d.ts`, so the bindings a module is linked against are the ones its
  generator read. It needs only the imports, their operations and
  signatures, the empties, and the handlers.
* **Bindings generators** read the rest. `generators/zena/` is the Zena
  generator, a separate crate that reads the file through the
  `ts-gen-abi` crate and nothing else from ts-gen.
  `examples/kotlin-worker/` has a Kotlin one, written in Kotlin with its
  own reader.

A generator for a language that cannot name its imports, as Kotlin's
`@JsFun` always imports from `js_code`, can still match the ABI's names
and pass `--bindings-module <name>` to `embed`, which then reads ABI
imports from that module and leaves its other imports to the host.
