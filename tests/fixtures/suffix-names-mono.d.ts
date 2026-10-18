//! @ts-gen --experimental-generic-mono

// Per-monomorphization overload suffixes: the arm the primary expansion
// takes at a position is implied (`fetch_with_init`), and a dictionary arm
// is named after its parameter (`_and_options`), unless that collides.
// Where two dictionary types meet at a position, only the one the other
// extends takes the parameter name (`head_with_options`), whichever comes
// first.
// A method's inline parameter type is named `<Parent><Method><Param>`.
// A `maybe` prefix is dropped (`json_with_init`), and a parameter named
// for its arms names each by its part (`measure_with_start_mark`). A
// method returning `this` returns the receiver.

interface RequestInit {
  method?: string;
}
interface ListenerOptions {
  once?: boolean;
}
declare class Request {
  constructor(input: Request | string, init?: RequestInit);
}
declare class Target {
  listen(type: string, handler: () => void, options?: ListenerOptions | boolean): void;
}
declare function fetch(input: Request | string | URL, init?: RequestInit): Promise<Request>;
declare class URL {
  constructor(url: string);
}

interface GetOptions {
  range?: number;
}
interface MeasureOptions {
  start?: string;
}
declare class Performance {
  measure(name: string, measureOptionsOrStartMark?: MeasureOptions | string, maybeEndMark?: string): void;
  session(constraintOrBookmark?: Bookmark | Constraint): void;
}
type Bookmark = string;
type Constraint = "first-primary" | "first-unconstrained";
declare class Response {
  static json(any: any, maybeInit?: RequestInit | Response): Response;
}
declare class Span {
  setAttribute(key: string, value: string): this;
}
declare class Bucket {
  get(key: string, options: GetOptions & { onlyIf: string }): void;
  put(key: string, options?: { md5?: string }): void;
  head(key: string, options: GetOptions & { onlyIf: string }): void;
  head(key: string, options?: GetOptions): void;
}
