//! @ts-gen --export tests/fixtures/provides-namespace.d.ts --provides tests/fixtures/provides-namespace.provides.d.ts
// Types for a module whose provided surface (provides-namespace.provides.d.ts)
// has a named function, a nested object and a default object: every JS
// path shape `--provides` can publish.
declare class Ticket {
  constructor(id: number);
  readonly id: number;
}
