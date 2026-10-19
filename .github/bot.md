ts-gen generates `#[wasm_bindgen] extern "C"` Rust bindings from TypeScript
`.d.ts` files. `src/parse` turns declarations into the IR in `src/ir.rs`, and
`src/codegen` turns the IR into Rust tokens. `AGENTS.md` describes the layout
and test layers in detail, and `CONVENTIONS.md` is the reference for how each
TypeScript construct translates.

- A change to generated output should come with a `CONVENTIONS.md` update
  when it changes a documented translation; undocumented snapshot churn is
  worth flagging.
- New constructs and output changes should be covered by a fixture in
  `tests/fixtures/*.d.ts` with its snapshot in `tests/snapshots/*.rs`
  (regenerated with `BLESS=1`), and new internal behaviour by unit tests in
  the relevant module.
- Generated bindings must compile against wasm-bindgen; behaviour that only
  shows up at runtime belongs in `integration-tests`.
- Oddities in input go through `DiagnosticCollector::warn`, not `println!`.
