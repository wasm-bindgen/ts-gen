// Per-mono a handle-or-primitive union returned with `| null` is `Option`
// of its dynamic-union enum, from a method or async member as from an
// optional getter. A method's overloads and a property of one name on
// several declarations share one enum; an unrelated member of the same
// shape gets its own (`ssec_key()` is `SsecKeyKind`, not `DataKind`).

export declare class File {
  readonly name: string;
}
export declare class FormData {
  get(name: string): (File | string) | null;
  first(): File | string;
  next(): Promise<File | string | null>;
  pick(index: number): File | string;
  pick(name: string): File | string;
}
export declare class MessageEvent {
  readonly data: string | ArrayBuffer;
  readonly previous?: string | ArrayBuffer;
}
export interface GetOptions {
  ssecKey?: ArrayBuffer | string;
}
export interface PutOptions {
  ssecKey?: ArrayBuffer | string;
}
export declare function options(): GetOptions | PutOptions;

// A list of such a union reads as a `Vec` of its enum, string literals
// included.
export declare class Entries {
  getAll(name: string): (File | string)[];
  readonly include?: ("httpMetadata" | "customMetadata")[];
}
