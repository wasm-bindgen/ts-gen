//! @ts-gen --lib-name export-star --export export-star

// `export *` is skipped with a warning, but its module still resolves.
export * from "./export-star/dep";
export declare function local(): void;
