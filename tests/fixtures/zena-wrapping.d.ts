//! @ts-gen --provides tests/fixtures/zena-wrapping.provides.d.ts
// Owner and string positions by the empties they admit, in both
// directions, for `--target zena`'s wrapping.
declare class Item {
  constructor();
  readonly name: string;
  label?: string;
  parent: Item | null;
}

declare function find(name: string | null, fallback?: Item): Item | null;
declare function first(): Item | undefined;
declare function title(): string | undefined;
declare function join(sep: string, ...parts: string[]): string;
