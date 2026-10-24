const locales = (value) =>
  value === undefined
    ? "none"
    : Array.isArray(value)
      ? `list(${value.join(",")})`
      : `${typeof value}(${value})`;

const options = (value) =>
  value === undefined
    ? "none"
    : `${value.constructor.name}${JSON.stringify(value)}`;

export const describe = (l, o) => `${locales(l)} ${options(o)}`;

export class Formatter {
  constructor(l, o) {
    this.received = describe(l, o);
  }
  format(l, o) {
    return describe(l, o);
  }
  locale(tag) {
    return new Intl.Locale(tag);
  }
}

export const settings = (value) =>
  `${locales(value.locales)} ${options(value.dateTime)}`;
