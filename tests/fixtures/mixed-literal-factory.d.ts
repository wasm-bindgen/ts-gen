//! @ts-gen --lib-name mixed-literal-factory --export mixed-literal-factory

// Setters take the getter's enum for the literals it keeps as variants,
// and factories pass those literals as variants.
export interface BoolOrStrings {
  mode: true | "x" | "y";
}

export interface NumberOrStrings {
  mode: 0 | "x" | "y";
}

export interface Mixed {
  mode: "x" | "y" | 1 | 2 | false;
}

// A union of integer literals is a C-style enum in both directions.
export interface Level {
  level: 0 | 1 | 2;
}

// String literals covered by `string` stay on the `&str` setter.
export interface Open {
  mode: "x" | string;
}

export interface Optional {
  mode?: "x" | "y";
}
