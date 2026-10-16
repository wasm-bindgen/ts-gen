const describe = (values) =>
  `${Array.isArray(values) ? "Array" : values.constructor.name}(${Array.from(values, String).join(",")})`;

export const generic = describe;
export class Bag {
  take(values) {
    return describe(values);
  }
}
