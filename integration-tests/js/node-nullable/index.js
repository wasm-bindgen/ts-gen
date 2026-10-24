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
export async function lookupAsync(key) {
  return lookup(key);
}
export async function find(key) {
  return key === "known" ? { label: "found" } : null;
}
export function classify(value) {
  return value === undefined ? "undefined" : value === null ? "null" : `value:${value}`;
}
export function options(label) {
  switch (label) {
    case "missing":
      return {};
    case "undefined":
      return { label: undefined };
    case "null":
      return { label: null };
    default:
      return { label };
  }
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
export function arity(...args) {
  return `${args.length}:${args.length ? String(args[args.length - 1]) : "-"}`;
}
export function pick(a, b) {
  return `${arguments.length}:${a === undefined ? "undefined" : a}:${b}`;
}
