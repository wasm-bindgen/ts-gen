# AGENTS

Guidance for AI agents and human contributors working in `ts-gen`.

## What this project does

`ts-gen` is a TypeScript-to-Rust binding generator. It reads `.d.ts`
files and emits Rust source containing `#[wasm_bindgen] extern "C"`
blocks. The output is meant to compile cleanly against `wasm-bindgen`
and let downstream Rust code interact with JS APIs that are described
purely by their TypeScript declarations.

## Project layout

```text
src/
├── lib.rs                     # public API (`parse_source`, re-exports)
├── ir.rs                      # IR types (TypeRef, ClassDecl, …)
├── context.rs                 # GlobalContext (scopes, registry, externals)
├── external_map.rs            # `--external Foo=::path::Bar` resolution
├── parse/                     # .d.ts → IR
│   ├── docs.rs                # JSDoc extraction (incl. @throws)
│   ├── members.rs             # interface / class member converters
│   ├── merge.rs               # var + interface merging
│   ├── classify.rs            # interface classification (class-like vs dictionary)
│   ├── types.rs               # TS type → IR TypeRef
│   ├── scope.rs               # scope / TypeId arena
│   ├── first_pass/            # two-phase population
│   │   ├── collect.rs         # phase 1: name registry
│   │   └── populate.rs        # phase 2: full IR
│   └── mod.rs                 # post-passes (merge_class_pairs, …)
├── codegen/                   # IR → Rust source
│   ├── mod.rs                 # entry, `generate(...)`
│   ├── classes.rs             # extern blocks for class-like types
│   ├── functions.rs           # free function / variable bindings
│   ├── enums.rs               # string + numeric enums
│   ├── signatures.rs          # parameter expansion + per-callable layer
│   ├── subtyping.rs           # builtin parents + `lub_types` (LUB)
│   └── typemap.rs             # TypeRef → syn tokens, `to_return_type`, externals
└── util/                      # naming, diagnostics, dedup helpers

tests/
├── fixtures/*.d.ts            # input declarations
├── snapshots/*.rs             # expected generated output (BLESS=1 to update)
└── snapshot.rs                # the snapshot harness

integration-tests/
├── tests/*.rs                 # hand-written integration tests
└── build.rs                   # generates per-test bindings via the library API
```

## Conventions

The single source of truth for "how does construct X translate?" is
[`CONVENTIONS.md`](CONVENTIONS.md). It catalogues every TypeScript →
Rust pattern we handle, ordered from simplest (primitives) to most
complex (subtyping LUB across unions).

**When to update `CONVENTIONS.md`:**

* Adding a new TS construct → add a numbered section.
* Changing an existing translation rule → update its section.
* Bug fix that changes user-visible output → update if the fix changes
  the documented behaviour, otherwise just add a snapshot test.

A PR that changes the codegen output without a corresponding
`CONVENTIONS.md` change is suspect — either the convention was missing
(add it) or the change is unintentional (revisit it).

## Code style

* Comments describe **nuance**, not the obvious. No `// ---` separators.
* Doc comments on public items use `///`; module-level docs use `//!`.
* Diagnostic-worthy oddities go through `DiagnosticCollector::warn`,
  not `println!`/`eprintln!`.
* Internal helpers are crate-private (`pub(crate)`) unless there's a
  reason to expose them to library consumers.

## Build / test recipes

The `justfile` is the single source for repeatable commands:

```sh
just test                # unit + snapshot tests
just test-overwrite      # bless snapshots after intentional output changes
just test-integration    # wasm-bindgen integration tests (needs wasm32 target)
just clippy              # workspace clippy with -D warnings
just fmt                 # apply rustfmt
just fmt-check           # CI rustfmt --check
```

All recipes use `cargo +stable` because the upstream workspace pins an
older toolchain via `rust-toolchain.toml`. CI uses the same.

## Tests

Three layers:

1. **Unit tests** (`cargo test --lib`) — focused, white-box checks of
   internal helpers (parse logic, naming, subtyping LUB, signature
   expansion). New behaviour should ship with unit tests in the
   relevant module's `#[cfg(test)] mod tests`.
2. **Snapshot tests** (`cargo test --test snapshot`) — fixture-driven.
   Every fixture under `tests/fixtures/` pairs with a snapshot under
   `tests/snapshots/`. Re-bless with `BLESS=1 cargo test`. Snapshot
   diffs in PRs should match documented convention changes — silently
   diff-only PRs are a smell.
3. **Integration tests** (`integration-tests/`) — actual wasm-bindgen
   compilation + browser/V8 execution. Used for end-to-end ABI
   correctness. Lives in its own crate that only builds for `wasm32`.

## Pipeline

```text
.d.ts files
  └─> oxc_parser (AST)
        └─> Phase 1: collect type names into TypeRegistry
              └─> Phase 2: populate full IR (resolve refs, merge var+iface, …)
                    └─> Post-passes (merge_class_pairs, classify, resolve imports)
                          └─> Codegen (CodegenContext, to_syn_type, emitters)
                                └─> syn::File → prettyplease → .rs
```

The two-phase parse is essential: phase 1 establishes name → kind
mappings so phase 2 can resolve forward references without re-walking
the AST.

## When in doubt

* **Are the conventions documented?** Check `CONVENTIONS.md` first; the
  pattern you're touching may already be specified there.
* **Is there a fixture?** Add one. A targeted `.d.ts` snippet in
  `tests/fixtures/` plus its rendered snapshot is the cheapest way to
  pin behaviour.
* **Does the change affect the rendered output?** Run
  `just test-overwrite` and review the snapshot diff carefully.
* **Is the change cross-cutting?** Likely belongs in `signatures.rs`
  (per-callable layer), `typemap.rs` (per-type), or `subtyping.rs`
  (lattice). If you're adding a new module, consider whether a helper
  in an existing one would do.
