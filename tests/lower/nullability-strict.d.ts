//! @ts-gen --lib-name nullability_strict --strict-nullable

// `--strict-nullable`: `T | null | undefined` is `T??`, read or written,
// and `T | null` is `@js(null) T?` as by default.
// `@tw-nullish` collapses a member to plain `T?`.

interface Init {
  signal?: Signal | null;
  /** @tw-nullish */
  body?: string | null;
}
declare class Signal {}
declare class Res {
  readonly body: string | null;
  /** @tw-nullish */
  readonly text: string | null;
  get(name: string | null): string | null;
  /** @tw-nullish */
  find(name: string | null): string | null;
}
// `null` at any depth keeps `@js(null)`, a future parameter's value too.
declare function nested(
  xs: Promise<string | null>,
  ys: Array<string | null>,
  r: Record<string, string | null>,
  cb: (x: string | null) => string | null,
): Promise<string | null>;
// An omittable parameter's omission is its `undefined`, so a trailing
// `x?: T | null` or `T | null | undefined` lowers like `T | undefined`,
// keeping only `@js(null)` under `--strict-nullable`.
declare function greet(name?: string | null): void;
declare function both(x: string | null | undefined): void;
declare function optional(x: string | undefined): void;
declare function lead(x: string | null | undefined, y: number): void;
declare function leadOpt(x?: string | null, y?: number): void;
