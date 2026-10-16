export function inspect(value) {
  return `inspect(${String(value)})`;
}
inspect.defaultOptions = { depth: 2 };
inspect.Formatter = class Formatter {
  constructor(depth) {
    this.depth = depth;
  }
  format(value) {
    return `${value}@${this.depth}`;
  }
};
inspect.withDepth = (depth) => new inspect.Formatter(depth);

export const outer = {
  inner: {
    LIMIT: 7,
    run: (style) => `run:${style}`,
  },
};
