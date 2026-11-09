# Reading 2: externref-native Rust

Each IR feature has a Rust mapping against the `tw` ABI directly, with no
wasm-bindgen. These mappings are documented here but not implemented.
Reading 1 (today) emits `#[wasm_bindgen]` from the IR, and the middle column
records where its choices are reading-1 only, so that they don't harden into
the IR.

`ExternRef` below is the language's owned `externref` handle, whatever the
Rust externref work names it. Classes are newtypes over it.

| IR feature (IR.md §) | Reading 1 (wasm-bindgen) | Reading 2 (externref-native) | Notes |
|---|---|---|---|
| `str` / `Str` (§3, §13) | `&str` / `String`, and `JsString` in generic arguments | `&str` in, `String` out over the linear profile's string layout. `Str` is a `JsString(ExternRef)` | Boxes stay newtypes. No copy for `Str` |
| Scalars, `i53`/`u53` (§10) | `f64`, `SafeInteger` | Native integers, validated at the bridge (§10 validation) | The bridge does the validation, so Rust needs no checks |
| `bigint` / `i64` / `u64` (§10) | `BigInt` / `i64` / `u64` | `i64` / `u64` scalars. `BigInt` is a box over `ExternRef` | — |
| `T?`, `T??` and `@js(null)` (§9) | `Option<T>`, with `JsValue::NULL` arms as wrappers | `Option<T>`, and `Option<Option<T>>` for `T??`. `null` vs `undefined` is the bridge's job, from `@js(null)` | The wrappers for null arms go away |
| Classes and interfaces (§4) | `extern type`, `extends`, getters and setters as methods | `#[repr(transparent)] struct C(ExternRef)`. Parents through `Deref`/`AsRef`. Members are imported functions taking `&C` | An interface stays structural: no check, unchecked casts |
| Namespaces and modules (§4, §5) | `js_namespace`, `module = "…"` | Import module fields, one Rust `mod` per namespace | — |
| Lib IRs and `with` (§5) | `js_sys` / `web_sys` paths from the external map | Lib crates generated from the lib IR (std as `tw-std`) and depended on by path | Externals keep mapping lib paths to crates |
| Overloads and `@js(args = …)` (§7) | One `fn` per overload, with `js_name` and `_with_…` suffixes | One import per overload. The bridge does the call shape (`args`, constants, rest) | Suffix naming carries over unchanged |
| `@js(name)`, `@js(object)`, `@js(this)` (§7) | `js_name`, dictionary `new()`, `this` receivers | The same as bridge facts. Rust sees plain functions | `@js(this = p)` is still reserved |
| Unit enums (§8) | String enums (`#[wasm_bindgen]` enum) | `#[repr(u32)] enum`, sent as an index. The bridge maps it to and from the literal | — |
| Payload enums (§8) | Expanded per arm, or taken whole as an enum with `Into` | A Rust `enum` whose arms carry their payloads, dispatched by the bridge's match | The whole-vs-expand choice is Rust ergonomics. Under reading 2 an enum is always whole |
| Flattened enums, inheritance, `@fallback` (§8) | Flattened arm lists, a `JsValue` fallback | Flattened `enum`, with a fallback arm `Other(ExternRef)` | — |
| Sequences `arr<T>`, `T[]`, tuples (§11) | `&[T]` / `Vec<T>`, typed-array copy overloads | `&[T]` / `Vec<T>` over the linear layout, tuples as Rust tuples | The copy overload (`u8[]`) next to a typed-array class is real in reading 2. Reading 1 drops it unless the parse paired it with an element array |
| Typed arrays and `view` (§11) | `Uint8Array` classes | Classes over `ExternRef`, plus borrowed views (`&[u8]` into JS memory via `view` intrinsics where the profile allows) | — |
| Views `*T[]`, `*mut T[]` (§11) | Read from the parse's `{@slice view}`; the IR's view overloads are skipped | `*const [T]` / `*mut [T]` | — |
| Mono generics (§13) | `experimental_generic_mono` | Monomorphised imports, one per instantiation. No runtime generics | — |
| Erased generics and bounds (§13) | `JsGeneric` type parameters | `PhantomData<T>` over `ExternRef`, with bounds as trait bounds | — |
| Associated types (§13) | Associated types on the generated trait | The same | — |
| Handles: callbacks, exported classes (§14) | `Closure` / `Function<fn(..)>` | A handle table entry. `@once` is `FnOnce`, and drop calls `tw:destroy` | Pairing gives `removeEventListener` identity for free |
| `throws E` (§15) | `catch` → `Result<T, JsValue>` or an error enum | `Result<T, E>`, with `E` the Rust enum of the throws arms. `err` is `JsError(ExternRef)` | — |
| `never` results (§3, §15) | `Result<(), E>` (always `Err`), the stable reading the parse emitted | `-> Result<!, E>`, or `-> !` with no `throws`, once `!` is stable | Reading 1 keeps `Result<(), E>`. Don't let `()` leak into the IR |
| `future<T, E>` (§16) | `Promise<T>` via `JsFuture` | `impl Future<Output = Result<T, E>>` over `tw:future`, polled by the cooperative loop | — |
| Cancellation, `@js(let …, cancel …)` (§16) | **Ignored**: the `let` and `cancel` entries are skipped, and a filled argument (`init? { signal = c.signal }`) is read at its own type, without `undefined` | Dropping the future calls the bridge's `drop`, which runs `cancel`. Rust needs nothing extra | Reading 1 can't cancel. That's acceptable until reading 2. Keep the call shape in the IR exact |
| `$result` cancel handles (§16) | Ignored | The same: the bridge holds `$result` | — |
| Exports, `export class` (§17) | `#[wasm_bindgen]` on Rust items | Language exports per §19, with Rust traits for `export class … extends` | — |
| Symbols (§3) | Well-known symbols as `Symbol`, the rest `JsValue` | `Symbol(ExternRef)` | — |
| `Any`, `Object` (§3) | `JsValue`, `Object` | `ExternRef`, `Object(ExternRef)` | — |

## Reading-1-only choices to keep out of the IR

- Cancel plumbing is ignored (`let`/`cancel`/filled arguments).
- `never` is read as `Result<(), E>`.
- Payload enums are expanded per arm unless taken whole (whole iff two or
  more payload arms, each a primitive or a non-generic bound class). Rust
  follows the IR's choice for a named union, an instance of a generic alias
  taken whole as an enum of its own (`LooseStr`); an anonymous union of
  values (`string | number`, `PropertyKey`) is expanded.
- Typed-array copy overloads are dropped unless an element array sits beside
  them.
- Overloads the parse merged, because they bind alike in wasm-bindgen, are
  emitted once.

## Known IR limits

- `@js(null)` marks any `T?` node (§9), so in a nested position (an
  element, a type argument, a callback parameter) a `| null` write crosses
  as `null` in reading 2 too, and reading 1 reads it from the IR, as
  `JsNullable`, a future parameter's value included.

## Parse-only readings

Reading 1 emits each type from the IR. The structure of a binding (its members,
its per-arm and per-overload expansions, and its `_with_…` names) stays
parse-driven, so it binds what the parse binds. Declarations and synthesised enums are
named as the parse spells them.

The IR states every type reading 1 emits; the facts it once couldn't
(`Symbol`, `NestedNull`, `UndefinedArg`, and recursive aliases, which it now
declares) were read from the parse at the same place until it did.

Defaults the IR fills in past the parse's arguments are left to the declaration.
An `any` argument whose declared default is open (`Fetcher<any>` for `Fetcher`)
is trimmed natively.
