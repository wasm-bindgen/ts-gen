//! @ts-gen --lib-name mixed-literal-factory --export mixed-literal-factory

// Literals of one kind share a setter, so every string literal in a
// mixed literal union calls the `&str` setter, not just the first.
export interface BoolOrStrings {
  mode: true | "x" | "y";
}

export interface NumberOrStrings {
  mode: 0 | "x" | "y";
}

export interface Mixed {
  mode: "x" | "y" | 1 | 2 | false;
}
