// Runtime for tests/fixtures/optional-reads.d.ts: absent, `null` and set
// fields read apart.
export class Moment {}
export class Tagged {}
export class Registry {
  static cleared = null;
  static anything = null;
}
export function settings(kind) {
  switch (kind) {
    case "empty":
      return { value: 1 };
    case "null":
      return { data: null, detail: null, cleared: null, value: null };
    default:
      return { data: 1, detail: "detail", stamp: 2, cleared: 3, maybe: true, value: "value" };
  }
}
