// Fixture: `declare var X: XConstructor`, the lib.es5 spelling of the
// var + interface class pattern. The constructor side is a named
// interface instead of an inline type literal; it merges the same way
// and the `*Constructor` interface itself is not emitted.

/** A named codec. */
interface Codec {
  readonly name: string;
  encode(input: string): Uint8Array;
}

interface CodecConstructor {
  new (name: string): Codec;
  (name: string): Codec;
  readonly prototype: Codec;
  /** Wire format version. */
  readonly version: number;
  isCodec(value: unknown): boolean;
}

declare var Codec: CodecConstructor;

// The var can precede the interfaces it merges with.
declare var Registry: RegistryConstructor;

interface RegistryConstructor {
  new (): Registry;
  readonly prototype: Registry;
}

interface Registry {
  lookup(name: string): Codec | undefined;
}

// A constructor-shaped interface with no matching var is an ordinary
// object type: `new` is not a constructor of `Factory` itself.
interface Factory {
  new (id: number): Widget;
  label: string;
}

interface Widget {
  id: number;
}
