//! @ts-gen --lib-name module-namespace-values --export module-namespace-values

// A namespace inside a module binds its value members through the
// namespace: `inspect.defaultOptions` is a property of the exported
// function, not an export of the module, so the static needs
// `module = "…"` and `js_namespace = inspect` together. This is the
// `util.inspect` / `util.promisify.custom` shape from @types/node.

export interface InspectOptions {
  depth: number;
}
export declare function inspect(value: unknown): string;
export declare namespace inspect {
  const defaultOptions: InspectOptions;
  function withDepth(depth: number): Formatter<string>;
  class Formatter<T> {
    format(value: T): string;
  }
  type Style = "plain" | "ansi";
}

export declare namespace outer {
  namespace inner {
    const LIMIT: number;
    function run(style: inspect.Style): string;
  }
}
