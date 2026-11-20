// An interface's optional method is an optional property of function
// type where the interface has no other method, which leaves it a
// dictionary. Beside a required method, under a parent with methods, on
// a class, and where generic or overloaded, it stays a method.

interface Element2 {
  tag: string;
}

interface Handlers {
  /** Called per element. */
  element?(element: Element2): void | Promise<void>;
  text?(chunk: string, last?: boolean): void;
  name?: string;
}
interface MoreHandlers extends Handlers {
  end?(): void;
}
declare function rewrite(handlers: Handlers, more: MoreHandlers): void;
declare function inline(handlers: { done?(): void; label: string }): void;
interface Stream {
  pipe(sink: { write?(chunk: string): void; close?(): void }): void;
}

interface Actor {
  fetch(request: string): string;
  alarm?(): void;
}
interface SubActor extends Actor {
  close?(): void;
}
declare class Entrypoint {
  fetch?(request: string): string;
}
interface Generic {
  pick?<T>(value: T): T;
}
interface Overloaded {
  get?(key: string): string;
  get?(key: number): number;
}
