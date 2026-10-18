// A side declaration carrying only `{@string …}` tags counts as tagged.

declare abstract class Headers {
  /** {@string ref} */
  keys(): IterableIterator<string>;
  /** {@string ref} */
  static readonly origin: string;
}

interface Body {
  /** {@string copy} */
  readonly contentType: string;
  /** {@string both} */
  text(): Promise<string>;
}

/** {@string copy} */
interface Cookies {
  /** {@string both} */
  readonly second: string;
}

declare namespace Tail {
  /** {@string copy} */
  interface Onset {}
}
