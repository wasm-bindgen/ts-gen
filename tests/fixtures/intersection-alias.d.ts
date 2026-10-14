// `type X = A & B & { … }` promotes to an interface extending every
// named part with the literal parts' members as its own. Parts without
// a structural reading (conditional types, parenthesised or not) are
// dropped with a warning; `object` and `unknown` add nothing. Alias
// type parameters carry onto the promoted declaration, for literal,
// union-literal and intersection targets alike.

interface Base {
  readonly colo: string;
}

interface Geo {
  readonly country?: string;
}

interface Response {
  readonly success: boolean;
}

type Properties<HostMetadata = unknown> = Base & Geo & { hostMetadata: HostMetadata };

type QueryResult<T = unknown> = Response & { results: T[] };

type Stub<T> = Response & (T extends string ? { named: true } : object);

type Endpoint<T> = (T extends string ? { named: true } : unknown) & {
  fetch(input: string): Promise<string>;
};

type Plain = Base & object & { extra: string };

type Range<R = any> = { offset?: number; value: R };

type ReadResult<R = any> = { done: false; value: R } | { done: true; value?: undefined };

// `Pick<V, …> & Omit<V, …>` both read as `V`, which is named once.
interface Vector {
  id: string;
  values: number[];
}
type Match = Pick<Vector, "id"> & Omit<Vector, "values"> & { score: number };
