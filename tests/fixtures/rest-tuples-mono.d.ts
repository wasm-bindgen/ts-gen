
// A tuple with a rest element has no fixed length: it is the array of its
// elements' common type. With none, a single head and an array rest are
// `ArrayHeadRest<H, R>`; any other shape is an array of `any`.

export declare function columnsAndRows<T>(): Promise<[string[], ...T[]]>;
export declare function names(): [string, ...string[]];
export declare function spread(): [number, ...[number, number]];
export declare function pair(): [string, number];
export declare function takeNames(names: [string, ...string[]]): void;
export declare function header(): [string, ...number[]];
export declare function takeHeader(row: [string, ...number[]]): void;
export declare function twoHeads(): [string, boolean, ...number[]];

export declare abstract class Statement {
  raw<T = unknown[]>(options: { columnNames: true }): Promise<[string[], ...T[]]>;
  raw<T = unknown[]>(options?: { columnNames?: false }): Promise<T[]>;
}
