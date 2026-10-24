// Runtime for tests/fixtures/optional-reads.d.ts: absent, `null` and set
// fields read apart.
export class Registry {
  static present = "present";
  static cleared = null;
}
export function settings(kind) {
  switch (kind) {
    case "empty":
      return { value: 1 };
    case "null":
      return { cleared: null, value: null };
    default:
      return { data: 1, detail: "detail", cleared: 2, maybe: true, value: "value" };
  }
}
