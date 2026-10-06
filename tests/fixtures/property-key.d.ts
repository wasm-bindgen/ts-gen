//! @ts-gen --lib-name property-key --export property-key

// Without per-monomorphization codegen, `PropertyKey` behaves exactly
// like `string | number | symbol`: arguments fan out per member and
// returns synthesise a dynamic-union enum.
export declare function hasKey(key: PropertyKey): boolean;
export declare function firstKey(): PropertyKey;
export declare function keys(): PropertyKey[];

// Primitive unions keep their per-member fan-out too.
export declare function setValue(value: string | number): void;
