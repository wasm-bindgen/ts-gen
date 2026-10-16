const describe = (values) =>
  `${Array.isArray(values) ? "Array" : values.constructor.name}(${Array.from(values, String).join(",")})`;

export class Item {
  constructor(name) {
    this.name = name;
  }
  toString() {
    return this.name;
  }
}

export const numbers = describe;
export const counts = describe;
export const literals = describe;
export const levels = describe;
export const strings = describe;
export const flags = describe;
export const bigints = describe;
export const items = describe;
export const generic = describe;
