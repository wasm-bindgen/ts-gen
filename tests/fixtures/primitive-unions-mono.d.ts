//! @ts-gen --experimental-generic-mono --lib-name primitive-unions-mono --export primitive-unions-mono

// Primitive unions spanning two or more categories become one parameter
// bounded by the matching `js_sys` marker trait.
export declare function setValue(value: string | number): void;
export declare function setFlag(value: boolean | bigint): void;
export declare function setAny(value: bigint | boolean | number | string | symbol): void;
export declare function setKeyed(value: string | symbol): void;

// Literals widen to their category.
export declare function setLevel(level: 1 | 2 | "auto"): void;

// Null / undefined arms drop in argument position, as for other unions.
export declare function setNullable(value: string | number | null): void;
export declare function setMaybe(value?: string | number): void;

// Aliases resolve before grouping.
export type Id = string | number;
export declare function lookup(id: Id): void;

// `PropertyKey` keeps its `js_sys::PropertyKey` spelling; the same
// members written out use the canonical trait name.
export declare function hasKey(key: PropertyKey): boolean;
export declare function hasKeyExplicit(key: string | number | symbol): boolean;

// Non-primitive members keep their own overloads next to the grouped one.
export interface Target {
  name: string;
}
export declare function send(to: Target | string | number): void;

// Single-category unions are unchanged.
export declare function setMode(mode: "a" | "b" | string): void;

// Returns keep the dynamic-union enum.
export declare function current(): string | number;

export declare class Store {
  constructor(key: PropertyKey);
  get(key: string | number): string;
  static of(value: boolean | number): Store;
}

export interface Settings {
  // Setters and factory arguments group; factory literals stay
  // constructors.
  level: "low" | "high" | number;
  id: string | number;
  label?: string | boolean;
  from: Target | string | boolean;
}
