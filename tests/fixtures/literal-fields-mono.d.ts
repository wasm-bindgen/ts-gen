//! @ts-gen --experimental-generic-mono

// Several literals within one shape are a choice of value: the factory takes
// the field as its setter does, rather than one factory per literal.
export type KeepAlive = 0 | 1 | 2;
export interface Level {
  level: 0 | 1 | 2;
  name: string;
}
export interface Revoked {
  revoked: "0" | "1";
  status: KeepAlive;
}
// The branches of a discriminated union each pin one literal, which names
// their factories.
export type Attachment =
  | { disposition: "inline"; content: string }
  | { disposition: "attachment"; content: string };
