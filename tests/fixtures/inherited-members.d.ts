// wasm-bindgen derefs a type to its first `extends =` parent only; the
// other parents are reachable through `AsRef` alone. Members of every
// parent beyond the first are therefore declared on the child itself,
// and a dictionary's factory sets the fields of all its parents. A type
// with no members of its own takes its classification from what it
// inherits (`Message` is a dictionary; `Named` is class-like).

interface Base {
  a: string;
}
interface Geo {
  country?: string;
}
interface Child extends Base, Geo {
  c: number;
}
interface Destinations {
  to?: string;
  cc?: string;
}
type Message = Base & Destinations;
interface Box<T> {
  value: T;
  unwrap(): T;
}
interface Tagged {
  tag: string;
}
interface Named extends Tagged, Box<string> {}
// Siblings that both declare a member: the earliest parent in `extends`
// order provides it; a redeclaration on the child wins over all.
interface Left {
  shared: string;
  onlyLeft(): void;
}
interface Right {
  shared: number;
  onlyRight(): void;
  own: string;
}
interface Siblings extends Base, Left, Right {
  own: number;
}
// A class parent beyond the first contributes its instance members only:
// its constructor and statics stay on its own JS global.
declare class Resource {
  constructor(id: string);
  static open(id: string): Resource;
  static readonly kind: string;
  readonly id: string;
  close(): void;
}
interface Handle extends Base, Resource {}
type Opened = Base & Resource;
