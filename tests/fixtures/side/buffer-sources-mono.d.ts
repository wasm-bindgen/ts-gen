interface Queue {
  /**
   * @param signature {@slice view}
   */
  verify(signature: BufferSource, data: BufferSource): boolean;
  /** @param view {@slice handle} the request's own buffer */
  respondWithNewView(view: ArrayBuffer | ArrayBufferView): void;
  /** @param value {@slice view} */
  put(key: string, value?: BufferSource): void;
}

/** @param data {@slice view} */
declare function digest(data: BufferSource): void;
