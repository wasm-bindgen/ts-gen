// An argument can't be `never`, so an overload with a `never` parameter is
// uncallable: it isn't lowered, and a module doesn't export it.
type Unit = "a" | "b";
declare function fallback(value: Exclude<Unit, Unit>): void;
declare function second(a: string, b: never): void;
declare module "m" {
  export function gone(x: never): void;
  export function kept(x: string): void;
}
