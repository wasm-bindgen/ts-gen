//! @ts-gen --lib-name slice-hints --export slice-hints

// A `{@slice view|array|copy}` JSDoc inline tag selects the wasm-bindgen
// slice mode for the parameter named by its enclosing `@param` (or for a
// property's setter value, when it appears in the property doc). `view`
// is never inferred: it is the author asserting the callee neither
// retains the buffer past the call nor re-enters wasm.

/**
 * Retains the received array for `first()` to read.
 * @param data {@slice view} consumed synchronously by the host
 */
export declare function keepView(data: Float64Array | number[]): void;

/** @param data {@slice copy} */
export declare function keepCopy(data: Float64Array | number[]): void;

/** Reads element 0 of the last retained array. */
export declare function first(): number;

/** @param values {@slice array} a plain `Array` instead of the typed copy */
export declare function entries(values: BigInt64Array | bigint[]): string;

// On a bare typed array, `copy` / `view` lowers to the element slice —
// "Rust callers hold native bytes here" — keeping the handle overload.
/** @param bytes {@slice copy} */
export declare function digest(bytes: Uint8Array): string;

// `Uint8Array<ArrayBuffer>` rejects a view of shared memory (WebIDL without
// `[AllowShared]`). An atomics build's wasm memory is shared, so there the
// `view` becomes a copy. The default `ArrayBufferLike` accepts either.
/** @param data {@slice view} */
export declare function keepUnsharedView(data: Uint8Array<ArrayBuffer>): void;

/** @param data {@slice view} */
export declare function keepSharedView(data: Uint8Array<ArrayBufferLike> | number[]): void;

export interface Target {
  /** The write destination. {@slice view} */
  buffer?: Float64Array | number[];
}

export declare function describeTarget(target: Target): string;
