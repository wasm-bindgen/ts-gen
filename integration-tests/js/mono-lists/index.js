const describe = (values) =>
  `${Array.isArray(values) ? "Array" : values.constructor.name}(${Array.from(
    values,
    (v) => `${typeof v}:${v}`,
  ).join(",")})`;

export const tags = describe;
export const tagged = describe;
export const tagsLike = describe;
export const flags = describe;
export const join = (separator, ...parts) => parts.join(separator);
export const sum = (...values) => values.reduce((a, b) => a + b, 0);
export class Bag {
  constructor(items) {
    this.items = items;
  }
  get size() {
    return this.items.length;
  }
  first() {
    return this.items[0];
  }
}
export const bagOf = (...items) => new Bag(items);
export const rows = (values) => values.map(describe).join(" ");
export const unique = (values) => describe([...values]);
export const lookup = (values, key) => values.get(key);
export const joinTags = (...tags) => tags.join(",");
export const awaitTag = () => "";
export const firstTag = (tags) => tags.next().value;
export const nextTag = () => "";
export const getTags = () => ["a", "b"];
export const getMaybeTags = () => undefined;
export const getFlags = () => [true, false];
export const fetchTags = async () => ["x"];
export const fetchMaybeTags = async () => ["y", "z"];
export const getValue = () => [true, false];
export class Labels {
  constructor() {
    this.values = [];
    this.flags = [];
  }
  add(...values) {
    return describe(values);
  }
}
export const describeOptions = (options) =>
  `${describe(options.values)} ${describe(options.flags)}`;
export class Total {
  constructor(...values) {
    this.value = values.reduce((a, b) => a + b, 0);
  }
}
