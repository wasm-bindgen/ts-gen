# ts-gen-zena

[Zena](https://github.com/elematic/zena) bindings for a Wasm ABI that
[ts-gen](../../README.md) derives from a `.d.ts`. The generator reads only
the ABI template, the annotated WAT file `ts-gen --target wat` writes and
[ABI.md](../../ABI.md) specifies, through the
[`ts-gen-abi`](../../abi/) crate. It depends on nothing else in ts-gen,
so it can live in Zena's own tree.

```sh
ts-gen --input worker.d.ts --output worker.abi.wat --target wat --provides ExportedHandler.fetch
ts-gen-zena worker.abi.wat -o bindings.zena
zena build main.zena -t js --imported-string-constants wasm:js/string-constants -o main.wasm
ts-gen embed --abi worker.abi.wat -m main.wasm -o out/
```

The bindings need a Zena compiler with `declare let` value imports and
`ref.null` emission for `externref | null`, which no Zena release has yet.
See [`examples/zena-worker/`](../../examples/zena-worker/) for a worker run
in workerd.

## Mapping


`ts-gen-zena` reads an ABI template (`ts-gen --target wat` output, see
[ABI.md](../../ABI.md)) and writes a `.zena` module importing the same
ABI, for Zena's `-t js` target. It is compiled with `--imported-string-constants
wasm:js/string-constants`, the namespace the merged module is loaded with.
Three layers:

* **Flat layer**: one `@external(<module>, <name>) declare function
  __<name>(...)` per import the `wat` template lists: each ABI function
  under its name and under each alias, from `./bindings.wasm` or its
  `declare module` specifier (whose identifier then prefixes the
  declaration's name), plus the `./builtins.js` `undefined` global as
  `declare let __undefined: externref` and `await` as `await_`. A
  reference is `externref` where TS never admits `null` and `externref |
  null` where it does: Zena keeps `null` out of `externref`, and both
  lower to the template's Wasm `externref`. Scalars are `f64`, `boolean`
  (lowers to `i32`) and `i64`. A variadic rest parameter is
  `FixedArray<anyref> | null`, the template's `(ref null $anyrefs)`.
  `tests/zena.rs` checks that these declarations and the template's
  imports are the same set, with the same Wasm types.
* **Sys layer**: wasm-bindgen's `JsValue` surface over `externref`,
  emitted once per file: `Undefined`, `isUndefined(x)`, `isEmpty(x)`
  (null or undefined) and `numberValue(x)`.
* **Class layer**: one `export class Type { #ref: externref; new(ref);
  ref(): externref; ... }` per owner, following the stdlib's host-object
  pattern. Members forward to the flat layer: getters are accessors,
  setters `set_x(value)`, constructors `new*` → `static create*()`,
  static members `static`. Declared members are named first, so a
  constructor yields `create` to a declared `static create` and becomes
  `create_2`. An owner reached only under an alias's name is `export type
  Alias = Type;`, so both names are one type and the static members are
  on `Type`. Free functions and global variables get an exported
  function of the same name. A module's owner or function whose name a
  global or an earlier one already has takes the specifier as a prefix
  (`declared_modules_os_constants` beside `"declared-modules/fs"`'s
  `constants`); a function's overloads take it
  together (`mail_send`, `mail_send_with_opt`).

Positions whose TS type is an owner, or a `string`, through aliases and
ignoring empty arms, are typed:

| TS position | Argument | Result |
| --- | --- | --- |
| `Type` | `Type`, passing `.ref()` | `new Type(r)` |
| `string` | `String`, passing `.handle` | `String.fromHandle(r)` |
| admits `null` | `Type \| null`, `null` passed as `null` | `Type \| null`, `null` for `null` (and `undefined` where admitted) |
| admits only `undefined` | `Type \| null`, `null` passed as `Undefined` | `Type \| null`, `null` for `undefined` |

An owner is looked up in the function's own module first, then among the
globals. On `-t js` a Zena `String` wraps the JS string, so strings
cross without a copy and Zena literals are JS string constants. Zena's
own `String` marshalling for `@external` passes the GC object, so the
flat layer keeps `externref`. Other references stay `externref` /
`externref | null`.

Identifiers: ABI names carry TS property keys verbatim, so they are
sanitised to `[A-Za-z0-9_]` (leading digit prefixed with `_`) and
deduplicated, parameters within each function too. Zena keywords (`new`,
`function`, `type`, ...) get a trailing `_`, the convention Zena's own
WIT bindings use. A class or function named like one the bindings
define or use (`Undefined`, `isUndefined`, `isEmpty`, `numberValue`,
`await_`, `String`, `Error`, `FixedArray`, `__r`, or a flat declaration)
takes a `_2` suffix.

For a template with handlers (`ts-gen --provides`), `main.zena` is
written next to the bindings, which it
imports by the output's file name (so `-o` cannot be `main.zena` or
`app.zena`): the compile entry, one raw export per handler wrapping its
wire values as above and calling the typed `app.zena`
(`app.fetch(new Request(request), new Env(env), new
ExecutionContext(ctx)).ref()`). An empty argument
reaches the app as `null`; an empty result goes back as `null`, or as
`Undefined` where TS admits only `undefined`. An `app.zena` scaffold
(JSDoc as comments, bodies that compile) is written only if absent. Zena
exports are identifiers, so a nested export path, or a name that is a
Zena keyword, is an error for this target. A `Promise<T>` handler is
written for JSPI: the app returns `T`.

Zena's `Error` imports `captureStackTrace` and `formatStackTrace` from
`env`; `embed` leaves them to the host, as it does every import outside
the ABI's modules.

### Limitations

## Tests

`cargo test -p ts-gen-zena` generates bindings from every fixture's
template in ts-gen's snapshot directory (`tests/snapshots/<name>/default.wat`)
and compares them with `tests/snapshots/` here, the way any consumer of
the format would run it. With `ZENA` set, it also compiles and embeds
each output and runs the example worker in node.
