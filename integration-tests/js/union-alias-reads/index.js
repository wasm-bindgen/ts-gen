// Runtime for tests/fixtures/union-alias-reads.d.ts.
export class Zoned {
  constructor(zone) {
    this.zone = zone;
  }
}
export class Day {
  constructor(day) {
    this.day = day;
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
