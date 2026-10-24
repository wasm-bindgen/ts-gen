//! @ts-gen --export tests/fixtures/alias-generics.d.ts --export shapes --export hidden --export uses

// Generic alias instantiations chase to their target with the arguments
// substituted (`Info<Meta>` → `Request<Meta> | string`), both as
// parameter unions and inside generic containers; a bare reference
// substitutes the parameter defaults. An alias whose lowered target no
// longer mentions a parameter is emitted without parameters. A qualified
// instantiation reads its target where it is written: `S.Tagged<string>`
// names `_Shape`, which `uses` reaches as `S.Shape`, one of the two names
// `shapes` exports it under.

interface Request<Meta = unknown> {
  readonly meta: Meta;
}

type Info<Meta = unknown> = Request<Meta> | string;

type Pair<A, B = string> = [A, B];

// A default naming an earlier parameter substitutes the argument given
// for it: `Same<number>` is `[number, number]`.
type Same<A, B = A> = [A, B];

type Boxed<T> = Array<T>;

declare class Client {
  send(input: Info<Request>): void;
  sendDefault(input: Info): void;
  pairs(): Promise<Pair<number>>;
  same(): Promise<Same<number>>;
  boxed(items: Boxed<string>): void;
}

declare module "shapes" {
  interface _Shape {
    readonly sides: number;
  }
  type Tagged<T> = [T, _Shape];
  export { _Shape as Shape, _Shape as Polygon, Tagged };
}

declare module "hidden" {
  interface Hidden {
    readonly h: number;
  }
  type Triple<X, Y = string> = [X, Y, Hidden];
  type Either<X> = X | Hidden;
  export { Triple, Either };
}

declare module "uses" {
  import * as S from "shapes";
  import * as H from "hidden";
  export function tag(t: S.Tagged<string>): void;
  // No path from `uses` reaches the unexported `Hidden`, so `H.Triple<number>`
  // is not chased and names the alias.
  export function triple(t: H.Triple<number>): void;
  // Nor is the union alias `H.Either<number>`, which then still names it.
  export function either(e: H.Either<number>): void;
}
