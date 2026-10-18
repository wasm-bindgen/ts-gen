//! @ts-gen --experimental-generic-mono --annotate tests/fixtures/side/buffer-sources-mono.d.ts

// A `&[u8]` passed as `impl BufferSourceLike` is copied unless its parameter
// is `{@slice view}`, inline or from the side file; `{@slice handle}` takes
// handles only.

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
