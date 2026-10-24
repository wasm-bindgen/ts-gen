//! @ts-gen --lib-name generic-hoisting --export generic-hoisting

// A parameter whose generic type has a top-type argument (a default,
// `unknown`, an erased union) takes a fresh type parameter in that
// position, so any instantiation passes without an upcast.
export type Loose = number | string;

export declare class Envelope<Meta = unknown, Body = Loose> {
  constructor(label: string);
  readonly label: string;
  // The method redeclares the class's parameters, so the hoisted ones
  // are numbered past them.
  forward(other: Envelope): string;
  // Returns keep the defaults.
  copy(): Envelope;
}

export declare class Courier {
  constructor(first: Envelope);
  send(envelope: Envelope): string;
  // Only the top-type position is hoisted: `Meta` stays `JsString`.
  sendLabelled(envelope: Envelope<string>): string;
  sendAsync(envelope: Envelope): Promise<string>;
  // Each union arm is its own parameter type.
  sendEither(envelope: Envelope | string): string;
  // Inside `Option` the defaults stay.
  sendMaybe(envelope: Envelope | undefined, tag: string): string;
  static check(envelope: Envelope): string;
}

export declare function post(envelope: Envelope): string;
export declare function postAsync(envelope: Envelope): Promise<string>;
// The callable's own `Meta` keeps its name.
export declare function relay<Meta>(envelope: Envelope, meta: Meta): string;
