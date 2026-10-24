globalThis.promiseOnlyTick = () => Promise.resolve("tick");

export const text = async () => "hello";
export const count = async () => 3;
export const flag = async () => true;
export const done = async () => undefined;
export const tags = async () => ["a", "b"];
export const maybeTags = async () => null;
export const rows = async () => [["a"], ["b", "c"]];
export const lookup = async (key) => (key === "k" ? "v" : null);
export const find = async (key) => (key === "k" ? "v" : undefined);
export const peek = async (key) => (key === "null" ? null : key === "k" ? "v" : undefined);
export const mode = async () => "slow";
export const anything = async () => 42;
export const identity = async (value) => value;

export function reject(message) {
  return Promise.reject(new Error(message));
}

export function throwNow(message) {
  throw new Error(message);
}

export function parse(input) {
  if (Number.isNaN(Number(input))) throw new TypeError(`malformed: ${input}`);
  return Promise.resolve(Number(input));
}

export const ready = async () => true;
export const size = async () => 7;
export const store = async (key, value) => `${key}=${value}`;
export const run = (callback) => callback();
export const boxed = async () => ({ value: "boxed" });

export class Item {
  constructor(name) {
    this.name = name;
    this.loaded = Promise.resolve(true);
  }
  async rename(name) {
    return new Item(name);
  }
  static async load(name) {
    return new Item(name);
  }
  async describe(prefix) {
    return prefix === undefined ? this.name : `${prefix}${this.name}`;
  }
}

export const items = async () => [new Item("x"), new Item("y")];
export const maybeItem = async (name) => (name ? new Item(name) : null);
