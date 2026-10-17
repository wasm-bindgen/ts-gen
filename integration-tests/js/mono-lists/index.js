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
export const getTags = () => ["a", "b"];
export const getMaybeTags = () => undefined;
export const getFlags = () => [true, false];
export const fetchTags = async () => ["x"];
export const getValue = () => [true, false];
export class Labels {
  constructor() {
    this.values = [];
    this.flags = [];
  }
}
export const describeOptions = (options) =>
  `${describe(options.values)} ${describe(options.flags)}`;
