// An intersection is a subtype of each of its members, so any member is
// an upper bound. Inline `{ ... }` literals lower to `object` and drop out,
// leaving the named member; between named members the subtype wins.
// `get`'s refined overload collapses onto `GetOptions` and dedups against
// the plain one, keeping the wider return type. Inside a union the member
// collapses first, so `(string & {})` is the plain `string` arm.

interface Conditional {
  etag?: string;
}
interface GetOptions {
  onlyIf?: Conditional;
  range?: string;
}
interface Base {
  id: string;
}
interface Derived extends Base {
  extra: string;
}
interface Unrelated {
  other: string;
}
interface Store {
  get(key: string, options: GetOptions & { onlyIf: Conditional }): Promise<string | null>;
  get(key: string, options?: GetOptions): Promise<string>;
  refine(value: Derived & Base): void;
  unrelated(value: Derived & Unrelated): void;
  shape(value: { a: string } & { b: number }): void;
}
type Refined = GetOptions & { onlyIf: Conditional };
interface Message {
  role: "user" | "assistant" | (string & {});
}
