//! @ts-gen --lib-name intl-types --export intl-types

// TypeScript's lib `Intl` namespace lowers onto `js_sys::Intl`: each type
// js-sys has (`Intl.DateTimeFormatOptions`, `Intl.Locale`, …) is its
// `::js_sys::Intl` type, and any other member stays unresolved. The lib
// aliases js-sys has no type for are built in: `Intl.LocalesArgument` fans
// out into a single locale (`&str`) and a list (`&[JsString]`), as js-sys's
// own bindings take locales, and `Intl.UnicodeBCP47LocaleIdentifier` is a
// `string`.

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
