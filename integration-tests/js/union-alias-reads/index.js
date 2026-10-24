// Runtime for tests/fixtures/union-alias-reads.d.ts.
export class Zoned {
  constructor(zone) {
    this.zone = zone;
  }
}

// The arm a value arrived as, so the tests can tell a class instance, a
// string and a plain dictionary apart.
const kind = (item) => {
  if (item === null) return "null";
  if (item === undefined) return "undefined";
  if (item instanceof Day) return "day";
  if (item instanceof Zoned) return "zoned";
  if (typeof item === "string") return "string";
  return "fields";
};

// The day an item stands for: a string is the number it spells, and a
// `Zoned` is day 0.
const ordinal = (item) => {
  if (typeof item === "string") return Number(item);
  if (item instanceof Zoned) return 0;
  return item.day;
};

export class Day {
  constructor(day) {
    this.day = day;
  }
  static from(item, options) {
    const day = new Day(ordinal(item));
    day.source = `${kind(item)}${options ? `:${options.overflow}` : ""}`;
    return day;
  }
  static compare(one, two) {
    return Math.sign(ordinal(one) - ordinal(two));
  }
  equals(other) {
    return ordinal(this) === ordinal(other);
  }
  until(other, options) {
    const days = ordinal(other) - ordinal(this);
    return options ? -days : days;
  }
  shift(offset) {
    return new Day(this.day + Number(offset));
  }
  step(direction, other) {
    return `${direction}:${kind(other)}`;
  }
  pick(...args) {
    return args.length === 0 ? "none" : kind(args[0]);
  }
  between(start, end) {
    return `${kind(start)}..${kind(end)}`;
  }
  async later(other) {
    return `later:${kind(other)}`;
  }
}

const values = {
  zoned: () => new Zoned("UTC"),
  day: () => new Day(3),
  string: () => "2020-01-01",
  zonedFields: () => ({ zone: "Europe/London" }),
  dayFields: () => ({ day: 4 }),
  bare: () => Object.assign(Object.create(null), { day: 5 }),
};
export function relativeTo(kind) {
  return values[kind]();
}
export function options(kind) {
  if (kind === "missing") {
    return { zone: "UTC", fields: { day: 1 } };
  }
  const value = relativeTo(kind);
  return {
    relativeTo: value,
    zone: value instanceof Zoned || typeof value === "string" ? value : "UTC",
    zoned: value instanceof Day ? undefined : value,
    fields: { zone: "UTC" },
  };
}

export const describe = (item) => `${kind(item)}:${ordinal(item)}`;
export const describeSpan = (span) =>
  `${kind(span.start)}->${"relativeTo" in span ? kind(span.relativeTo) : "none"}`;
export const describeFields = (fields) =>
  "zone" in fields ? `zone:${fields.zone}` : `day:${fields.day}`;
export const either = (item) => kind(item);
