//! @ts-gen --lib-name builtin-errors --export builtin-errors

// Fixture: js_sys error classes resolve as `@throws` types without a
// declaration or `--external`. `URIError` is spelled `UriError` in
// js_sys and gets aliased; `AggregateError` comes from the glob. The
// `try_` forms return them as the error type; the union collapses to
// the common ancestor `Error`.

export declare class Codec {
  /** @throws {URIError} on a malformed escape sequence */
  decode(input: string): string;

  /** @throws {AggregateError} when several escape sequences are malformed */
  decodeAll(input: string): string;

  /** @throws {URIError | TypeError} */
  parse(input: string): Promise<string>;
}
