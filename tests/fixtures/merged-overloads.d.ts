//! @ts-gen --input tests/fixtures/merged-overloads/b.d.ts

// A method's overloads accumulate across every merged declaration of its
// class or interface, the later declaration's first, as TypeScript's
// overload resolution tries them. Constructors and statics accumulate the
// same way; a property stays one binding.

interface Stream {
  write(data: string): void;
  write(data: number): void;
}
interface Stream {
  write(data: boolean): void;
  // The same overload again is the same binding.
  write(data: string): void;
  readonly size: number;
}
interface Stream {
  readonly size: number;
}

declare class Widget {
  draw(x: string): void;
}
interface Widget {
  draw(x: number): void;
}

// The class is the later declaration, so its overload is first.
interface Gadget {
  run(x: number): void;
}
declare class Gadget {
  run(x: string): void;
}

// The later declaration's return type wins for the same parameters.
interface Clock {
  now(x: string): number;
}
interface Clock {
  now(x: string): Date;
}

interface Codec {
  encode(input: string): string;
}
interface CodecConstructor {
  new (name: string): Codec;
  readonly prototype: Codec;
  from(input: string): Codec;
}
interface CodecConstructor {
  new (id: number): Codec;
  from(input: ArrayBuffer): Codec;
}
declare var Codec: CodecConstructor;

// `put`'s `number` form is in b.d.ts, the later file, so it is first.
interface Box {
  put(x: string): void;
}
