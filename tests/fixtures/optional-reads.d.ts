//! @ts-gen --lib-name optional-reads --export optional-reads
// A property that erases to `JsValue` reads as `Option` when it admits an
// empty: an absent field and a written `null` are both `None` (the `strict`
// snapshot keeps the `Option` for absence only, with a written `null` as the
// `Nullable` under it). The top type already holds both empties in-band, so
// `any` and `unknown` stay bare. Exercised against a real JS module in the
// integration tests.

export declare class Moment {
  constructor();
}

export declare class Tagged {
  constructor();
}

export interface Settings {
  /** Anything, `null`, or absent: all in-band. */
  data?: any;
  /** Anything, `null`, or absent: all in-band. */
  detail?: unknown;
  /** No member subtypes the other, so it erases. */
  stamp?: Moment & Tagged;
  /** Erased, `null`, or absent. */
  cleared?: (Moment & Tagged) | null;
  /** Required, but admitting `undefined`. */
  maybe: (Moment & Tagged) | undefined;
  /** Always present, so bare. */
  value: Moment & Tagged;
}

export declare class Registry {
  /** Absent. */
  static missing?: Moment & Tagged;
  /** Present, holding `null`. */
  static cleared?: (Moment & Tagged) | null;
  /** The top type, holding `null`. */
  static anything?: any;
}

/** Settings with every field set (`"full"`), none (`"empty"`) or `null`s. */
export declare function settings(kind: string): Settings;
