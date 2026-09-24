//! @ts-gen --experimental-generic-mono --lib-name generic-mono-review --export generic-mono-review

export type Id = string;

export declare class S {
  readonly x: number;
}

export declare class S2 {
  readonly x: number;
}

export declare function byAlias(id: Id): Id;
export declare function asyncByAlias(id: Id): Promise<Id>;
export declare function withS(s: S, s2: S2, name: string): void;

export declare class Holder<T> {
  constructor(value: T);
  get(): T;
}

export declare function takesHolder(h: Holder<Id>, g: Holder<string>): void;
export declare function holderOfId(): Holder<Id>;
