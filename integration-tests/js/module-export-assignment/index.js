// The ESM shape of a CommonJS `module.exports = check` whose properties are
// exposed as named exports, as Node does for `node:assert`.
export default function check(value) {
  if (!value) throw new Error("check failed");
}
export function strictEqual(actual, expected) {
  return actual === expected;
}
export const VERSION = "1.0";
export const nested = { run: () => "nested" };
