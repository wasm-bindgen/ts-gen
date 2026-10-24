//! @ts-gen --lib-name optional-reads --export optional-reads
// A property that can be absent reads as `Option`, even when its value
// erases to `JsValue`, and a written `null` as the `Nullable` under it.
// Exercised against a real JS module in the integration tests.

export interface Settings {
  /** Anything, or absent. */
  data?: any;
  /** Anything, or absent. */
  detail?: unknown;
  /** Anything, `null`, or absent. */
  cleared?: unknown | null;
  /** Required, but admitting `undefined`. */
  maybe: unknown | undefined;
  /** Always present, so bare. */
  value: unknown;
}

export declare class Registry {
  /** Absent. */
  static missing?: unknown;
  /** Present. */
  static present?: unknown;
  /** Present, holding `null`. */
  static cleared?: any | null;
  readonly extra?: any;
}

/** Settings with every field set (`"full"`), none (`"empty"`) or `null`s. */
export declare function settings(kind: string): Settings;
