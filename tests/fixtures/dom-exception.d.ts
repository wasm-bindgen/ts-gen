// Fixture: `DOMException` resolves to `web_sys::DomException` through
// the web platform defaults, without a declaration or `--external`.

declare class Codec {
  /** @throws {DOMException} when the codec is closed */
  encode(input: string): Uint8Array;
}
