// Runtime for tests/fixtures/node-nullable.d.ts: null and undefined differ.
export function describe(name = "anon") {
  return name === null ? "no name" : `name:${name}`;
}
export function repeat(count = 1) {
  return "x".repeat(count);
}
export function greet(name = "world") {
  return name === null ? "hello nobody" : `hello ${name}`;
}
export function lookup(key) {
  return key === "empty" ? undefined : key === "known" ? "value" : null;
}
export function configure(options) {
  return "label" in options
    ? options.label === null
      ? "unlabelled"
      : options.label === undefined
        ? "label omitted"
        : `label:${options.label}`
    : "no label key";
}
