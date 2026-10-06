//! @ts-gen --lib-name symbol --export symbol

// `symbol` lowers to `js_sys::Symbol` in every position.
export declare function describe(value: symbol): string;
export declare function create(description?: string): symbol;
export declare function find(name: string): symbol | null;
export declare function all(): symbol[];
export declare function forEach(callback: (value: symbol) => void): void;

// Unions fan out to a `_with_symbol` overload; returns get a `Symbol`
// variant.
export declare function setKey(key: string | symbol): void;
export declare function lookup(): string | symbol;

export interface Tagged {
  tag: symbol;
  alias?: symbol;
}
