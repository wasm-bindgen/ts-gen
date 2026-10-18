//! @ts-gen --experimental-generic-mono

// A tuple with a rest element has no fixed length: it is the array of its
// elements' common type, or of `any`.

export declare function columnsAndRows<T>(): Promise<[string[], ...T[]]>;
export declare function names(): [string, ...string[]];
export declare function spread(): [number, ...[number, number]];
export declare function pair(): [string, number];
export declare function takeNames(names: [string, ...string[]]): void;
