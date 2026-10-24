//! @ts-gen --lib-name builtin-errors --export builtin-errors
//! @ts-gen --external Intl.NumberFormatOptions=::js_sys::Object

// Fixture: names the lib declares that js_sys provides under another
// spelling resolve through the external map's js-sys defaults, without a
// declaration or `--external`, and with or without `--no-web-sys`.

// The js_sys error classes resolve as `@throws` types. `URIError` is
// spelled `UriError` in js_sys and gets aliased; `AggregateError` comes
// from the glob. The `try_` forms return them as the error type; the union
// collapses to the common ancestor `Error`.
export declare class Codec {
  /** @throws {URIError} on a malformed escape sequence */
  decode(input: string): string;

  /** @throws {AggregateError} when several escape sequences are malformed */
  decodeAll(input: string): string;

  /** @throws {URIError | TypeError} */
  parse(input: string): Promise<string>;
}

// The lib's `Intl` namespace: each type js-sys has
// (`Intl.DateTimeFormatOptions`, `Intl.Locale`, …) maps by its qualified
// path to its `::js_sys::Intl` type, and any other member stays
// unresolved. The `--external` mapping of `Intl.NumberFormatOptions` above
// beats the default. The lib aliases js-sys has no type for are built in:
// `Intl.LocalesArgument` fans out into a single locale (`&str`) and a list
// (`&[JsString]`), as js-sys's own bindings take locales, and
// `Intl.UnicodeBCP47LocaleIdentifier` is a `string`.
export declare class Formatter {
  constructor(locales?: Intl.LocalesArgument, options?: Intl.DateTimeFormatOptions);
  readonly received: string;
  format(locales?: Intl.LocalesArgument, options?: Intl.NumberFormatOptions): string;
  locale(tag: Intl.UnicodeBCP47LocaleIdentifier): Intl.Locale;
}

export declare function describe(
  locales?: Intl.LocalesArgument,
  options?: Intl.DateTimeFormatOptions,
): string;

export interface FormatSettings {
  locales?: Intl.LocalesArgument;
  dateTime?: Intl.DateTimeFormatOptions;
}

export declare function settings(value: FormatSettings): string;
