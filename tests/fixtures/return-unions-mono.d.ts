//! @ts-gen --experimental-generic-mono

// Per-mono a handle-or-primitive union returned with `| null` is `Option`
// of its dynamic-union enum, from a method or async member as from an
// optional getter.

export declare class File {
  readonly name: string;
}
export declare class FormData {
  get(name: string): (File | string) | null;
  first(): File | string;
  next(): Promise<File | string | null>;
}
export declare class MessageEvent {
  readonly data: string | ArrayBuffer;
  readonly previous?: string | ArrayBuffer;
}
