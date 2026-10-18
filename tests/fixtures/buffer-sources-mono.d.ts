//! @ts-gen --annotate tests/fixtures/side/buffer-sources-mono.d.ts

// A `&[u8]` passed as `impl BufferSourceLike` is copied unless every buffer
// source on the import is `{@slice view}`; `{@slice handle}` takes handles
// only. The mode sits on the fn, as wasm-bindgen applies it only to the
// slice instantiation.

type BufferSource = ArrayBufferView | ArrayBuffer;

declare abstract class Queue {
  enqueue(chunk: ArrayBuffer | ArrayBufferView): void;
  /** @param data {@slice view} the queue copies the data before returning */
  write(data: BufferSource, offset?: number): void;
  verify(signature: BufferSource, data: BufferSource): boolean;
  /** @param plainText {@slice view} */
  encrypt(plainText: BufferSource): void;
  respondWithNewView(view: ArrayBuffer | ArrayBufferView): void;
  put(key: string, value?: BufferSource): void;
}

declare function digest(data: BufferSource): void;

interface Options {
  md5?: BufferSource;
}
