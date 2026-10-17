//! @ts-gen --lib-name alias-constructors --export alias-constructors --export tests/fixtures/alias-constructors.d.ts

// A constructor-shaped `const` sharing its name with a type alias, as with
// workers-types' `RpcStub`, binds its construct signatures as constructors
// of the types they return, and the alias stays the type.

export interface Stub<T> {
  get(): T;
}

export type Wrap<T> = Stub<T>;
export declare const Wrap: { new <T>(value: T): Stub<T> };

export declare function unwrapText(stub: Wrap<string>): string;

// Declared after the `const`, with overloads.
export declare const Label: {
  new (text: string): Labelled;
  new (n: number, unit: string): Labelled;
};
export type Label = Labelled;

export interface Labelled {
  text(): string;
}

// A dictionary has its own `new` builder, as does a type another variable
// already constructs, so these bind as values.
export interface Point {
  x?: number;
}
export type Pt = Point;
export declare const Pt: { new (): Point };

export type Wrap2<T> = Stub<T>;
export declare const Wrap2: { new <T>(value: T): Stub<T> };

// A literal of other members than construct signatures and `prototype`
// keeps them reachable through the value, as does a class with a member
// named `new`.
export interface Proto {
  p(): void;
}
export type ProtoOnly = Proto;
export declare const ProtoOnly: { prototype: Proto };

export type WithStatics<T> = Wrapped<T>;
export declare const WithStatics: { new <T>(value: T): Wrapped<T>; helper(): number };
export interface Wrapped<T> {
  get(): T;
}

export declare class StaticNew {
  static new(): StaticNew;
  y(): void;
}
export type SN = StaticNew;
export declare const SN: { new (): StaticNew };

export interface CapNew {
  New(): void;
}
export type CN = CapNew;
export declare const CN: { new (): CapNew };

// The `const` is exported where its alias isn't, in a module that has
// only its explicit exports.
type Count = Counter;
export declare const Count: { prototype: Counter; new (start: number): Counter };
export interface Counter {
  count(): number;
}

// Each constructed type takes its own constructor names.
export type Multi = MultiA;
export declare const Multi: { new (): MultiA; new (n: number): MultiB };
export interface MultiA {
  a(): string;
}
export interface MultiB {
  b(): number;
}

export {};
