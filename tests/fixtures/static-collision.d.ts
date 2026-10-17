// Fixture: a static member and an instance member with the same JS name
// share one extern block. The instance member keeps the base name
// regardless of declaration order; the static one takes `_static`, as in
// MDN's `Response/json_static`. Declared instance names also beat
// synthesized companions (`try_`, `_static`) that would spell the same.

declare class Response {
  static json(data: any, init?: ResponseInit): Response;
  static error(): Response;
  json(): Promise<any>;
  readonly ok: boolean;
  static readonly ok: boolean;
}

interface ResponseInit {
  status?: number;
}

declare class Counter {
  count(): number;
  try_count(): number;
}
