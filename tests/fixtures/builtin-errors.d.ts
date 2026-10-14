// Fixture: error classes that TS spells differently from their Rust
// binding resolve without a declaration or `--external`. `URIError` is
// `js_sys::UriError`; `DOMException` is `web_sys::DomException` through
// the web platform defaults. Both are bindable `@throws` names, so the
// `try_` forms return them as the error type; the union collapses to
// their common ancestor `Error`.

declare class Codec {
  /** @throws {URIError} on a malformed escape sequence */
  decode(input: string): string;

  /** @throws {DOMException} when the codec is closed */
  encode(input: string): Uint8Array;

  /** @throws {URIError | TypeError} */
  parse(input: string): Promise<string>;
}
