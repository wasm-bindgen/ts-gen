// A required field the IR doesn't write, such as a `never`, leaves its
// dictionary without factories or a builder, which would build it
// without the field; the field is warned. An optional one is left out of
// the builder.

interface Sealed {
  label: string;
  seal: never;
}

interface Unsealed {
  label: string;
  seal?: never;
}
