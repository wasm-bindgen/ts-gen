// Per-monomorphization overload suffixes: the arm the primary expansion
// takes at a position is implied (`fetch_with_init`), and a dictionary arm
// is named after its parameter (`_and_options`), unless that collides.

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
