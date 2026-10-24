//! @ts-gen --lib-name mono-lists --export mono-lists

// Every string leaf of a list or built-in container argument accepts any
// string shape, variadics included, and a string list return gets a
// `_js_string` variant returning the list as a handle, as a string return
// does. Boolean lists cross as `bool`.

export type Tag = string;
export declare function tags(values: string[]): string;
export declare function tagged(values: Tag[]): string;
export declare function tagsLike(values: ArrayLike<string>): string;
export declare function flags(values: boolean[]): string;
export declare function join(separator: string, ...parts: string[]): string;
export declare function sum(...values: number[]): number;
// A rest array infers its element, so `T` survives erasure.
export declare function bagOf<T>(...items: T[]): Bag<T>;
export declare class Bag<T> {
  readonly size: number;
  first(): T;
}
export declare function rows(values: string[][]): string;
export declare function unique(values: ReadonlySet<string>): string;
export declare function lookup(values: Map<string, number>, key: string): number;
export declare function joinTags(...tags: Tags): string;
export declare function awaitTag(tag: Promise<string>): string;
export declare function firstTag(tags: Iterator<string>): string;
export declare function nextTag(tags: AsyncIterator<string>): string;
export type Tags = string[];
export declare function getTags(): string[];
export declare function getMaybeTags(): Tag[] | undefined;
export declare function getFlags(): boolean[];
export declare function fetchTags(): Promise<string[]>;
export declare function fetchMaybeTags(): Promise<Tag[] | null>;
export declare class Labels {
  constructor();
  add(values: string[]): string;
  add(...values: string[]): string;
  values: string[];
  flags: boolean[];
}
export interface LabelOptions {
  values?: string[];
  flags?: boolean[];
}
export declare function describeOptions(options: LabelOptions): string;
export declare function getValue(): string | boolean[];
export declare class Total {
  constructor(...values: number[]);
  readonly value: number;
}
