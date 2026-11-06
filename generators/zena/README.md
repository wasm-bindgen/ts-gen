# ts-gen-zena

[Zena](https://github.com/elematic/zena) bindings for the ts-gen IR, the
language-neutral form [ts-gen](../../README.md) expands a `.d.ts` into
([IR.md](../../IR.md)). The generator reads only the IR text, through the
[`ts-gen-abi`](../../abi/) crate. It depends on nothing else in ts-gen,
so it can live in Zena's own tree.

```sh
ts-gen --input worker.d.ts --output worker.ir --target ir --naming zena --provides ExportedHandler.fetch
ts-gen-zena worker.ir -o bindings.zena
zena build main.zena -t js --imported-string-constants wasm:js/string-constants -o main.wasm
ts-gen embed --ir worker.ir -m main.wasm -o out/
```

`--naming zena` names the IR's bindings for Zena: constructors are
`create` / `create_with_*`; a member, class, kind, arm or parameter that
is `new`, `ref` or a Zena keyword gets a trailing `_`; a character
outside `[A-Za-z0-9_$]` is written `_u<hex>_` (`déjà` is
`d_ue9_j_ue0_`); and collisions are resolved per scope. `ts-gen-zena`
takes names verbatim, so an IR written with another preset still
generates, but may not compile.

The bindings need a Zena compiler with `declare let` value imports and
`ref.null` emission for `externref | null`, which no Zena release has yet.
See [`examples/zena-worker/`](../../examples/zena-worker/) for a worker run
in workerd.

## Mapping

`ts-gen-zena` writes a `.zena` module importing the IR's bindings, for
Zena's `-t js` target. It is compiled with `--imported-string-constants
wasm:js/string-constants`, the namespace the merged module is loaded
with. Three layers:

* **Flat layer**: one `@external(<module>, "<class>.<member>") declare
  function __<name>(...)` per IR member (`<member>` alone for a
  top-level one), from `./bindings.wasm`, the class's `module`, or the
  member's own `from` module, which also prefixes the declaration's name.
  Each is commented with the member as the IR writes it, and each class's
  first with its header (`shape class ExportedHandler<E>`). An instance
  member takes the receiver first. A reference is `externref` where the
  position never admits `null` and `externref | null` where it does
  (Zena keeps `null` out of `externref`; both lower to Wasm
  `externref`). `f64`, `bool` (as `boolean`, lowering to `i32`) and
  `i64` are themselves. A variadic rest parameter is `FixedArray<anyref>
  | null`, the IR's `(ref null $anyrefs)`. Plus the three functions every
  `bindings.wasm` has (`JsValue.is_undefined`, `JsValue.undefined`,
  `Number.value`), the `./builtins.js` `undefined` global as `declare
  let __undefined: externref`, and `await` as `await_`. Member aliases
  are not imported: an alias calls its member's declaration.
  `tests/zena.rs` checks that these declarations are the IR's imports,
  with the same Wasm types.
* **Sys layer**: wasm-bindgen's `JsValue` surface over `externref`,
  emitted once per file: `Undefined`, `isUndefined(x)`, `isEmpty(x)`
  (null or undefined) and `numberValue(x)`.
* **Class layer**: one `export class Type { #ref: externref; new(ref);
  ref(): externref; ... }` per IR class, following the stdlib's
  host-object pattern; a `static class` with no members, which only holds
  nested paths, has none. Members forward to the flat layer under their
  IR names and aliases: getters without parameters are accessors,
  setters and getters with parameters are methods, constructors and
  `new {}` are static factories, static members `static`. The instance
  members of the classes a class `extends`, transitively, are presented
  on it too, unless it has a member of that name, with the base's type
  arguments substituted (`Named extends Box<String>` reads `value` as a
  `String`). A class alias is `export type Alias = Type;`. Top-level
  members, getters included, are exported functions, one per name and
  alias. A kind is `export type Kind = W;`, `W` the wrapper all its
  arms share (`String` for a string enum, a class), else
  `externref`. Each kind also gets its discriminator, `@external(<module>,
  "kind:<kind>") declare function __kind_<kind>(x: externref | null):
  i32`, and a match surface over it (below).

A kind's value, from a result or a provide parameter, is matched with
`<Kind>_arm(x)`, which asks the discriminator for the arm (IR.md,
Recognition) and returns a case of `export sealed class <Kind>_Arm`: a
literal arm is a unit case, `<Kind>_<Arm>`, and another arm a case
holding the value as its position would be typed (`Number` as `f64`):

```zena
match (HeadersKind_arm(init)) {
  case let HeadersKind_Headers { value as h }: ...
  case let HeadersKind_VecOfTuple { value as pairs }: ...
  case let HeadersKind_Record { value as record }: ...
}
```

A value of no arm throws, as Rust's binding does; in a `fallback` kind
the last non-literal arm is the match's default. A position's empties are
tested before, as its type says.

The binding names are unique per module; Zena has one namespace per
file. Classes, kinds and class aliases are named first, then top-level
members. A dotted path becomes `_`-joined (`N.Foo` is `N_Foo`). A
module's name that a global or earlier name already has takes the
specifier as a prefix (`mail_send` beside a global `send`), the
overloads of one function together. A name the bindings define or use
(`Undefined`, `isUndefined`, `isEmpty`, `numberValue`, `await_`,
`String`, `Error`, `FixedArray`, `__r`, or a flat declaration) takes a
`_2` suffix, as does a parameter named `this_` or `__r`. The match
surface's names are taken last, with the same suffix on a clash.

Positions whose type is a class, a string or a string literal, or a kind
whose arms share one of those, are typed:

| IR position | Argument | Result |
| --- | --- | --- |
| `Type` | `Type`, passing `.ref()` | `new Type(r)` |
| `String` | `String`, passing `.handle` | `String.fromHandle(r)` |
| admits `Null` | `Type \| null`, `null` passed as `null` | `Type \| null`, `null` for `null` (and `undefined` where admitted) |
| admits only `Undefined` | `Type \| null`, `null` passed as `Undefined` | `Type \| null`, `null` for `undefined` |

On `-t js` a Zena
`String` wraps the JS string, so strings cross without a copy and Zena
literals are JS string constants. Zena's own `String` marshalling for
`@external` passes the GC object, so the flat layer keeps `externref`.
Other references (`Any`, `Number`, `Promise<T>`, type parameters,
callbacks, tuples) stay `externref` / `externref | null`, or the kind's
alias. Type arguments, `throws`, `raises` and `nothrow` appear only in
the comments: the bridge does not report caught exceptions to the
language yet.

For an IR with provides, `main.zena` is written next to the bindings,
which it imports by the output's file name (so `-o` cannot be
`main.zena` or `app.zena`): the compile entry, one raw export per
provide wrapping its wire values as above and calling the typed
`app.zena` (`app.fetch(new Request(request), new Env(env), new
ExecutionContext(ctx)).ref()`). An empty argument reaches the app as
`null`; an empty result goes back as `null`, or as `Undefined` where
only `undefined` is admitted. An `app.zena` scaffold (docs as comments,
bodies that compile) is written only if absent. Zena exports are
identifiers, so a nested export path, or a name that is a Zena keyword,
is an error for this target. An `async` provide is written for JSPI: the
app returns the value itself, and an `async void` one returns nothing,
its export `Undefined`.

Zena's `Error` imports `captureStackTrace` and `formatStackTrace` from
`env`; `embed` leaves them to the host, as it does every import outside
the IR's modules.

## Tests

`cargo test -p ts-gen-zena` builds the IR of every fixture in ts-gen's
`tests/fixtures/` with its `//! @ts-gen` options and `--naming zena`,
reads it back from its text, generates bindings and compares them with
`tests/snapshots/` here. With `ZENA` set, it also compiles each output,
embeds it with `ts-gen embed --ir`, and runs the example worker in node.
