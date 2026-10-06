//! @ts-gen --experimental-generic-mono --lib-name generic-mono --export generic-mono

export declare class Holder<T> {
  constructor(value: T);
  get(): T;
  set(value: T): void;
}

export declare function identity<T>(value: T): T;

export interface EvaluationDetails<T> {
  flagKey: string;
  value: T;
  variant?: string | undefined;
}

export interface Pair<A, B> {
  first: A;
  second: B;
}

export interface TypedArrayOptions {
  value: ArrayBufferView;
}

export declare class Flags {
  getStringValue(defaultValue: string): Promise<string>;
  getStringDetails(defaultValue: string): Promise<EvaluationDetails<string>>;
  getBooleanDetails(defaultValue: boolean): Promise<EvaluationDetails<boolean>>;
  getNumberDetails(defaultValue: number): Promise<EvaluationDetails<number>>;
  getNullableString(defaultValue: string): Promise<string | undefined>;
  getStringPair(defaultValue: string): Promise<Pair<string, string>>;
  getStringArray(): Promise<Array<string>>;
  getStringMap(): Promise<Map<string, string>>;
  getStringTuple(): Promise<[string, string]>;
  getStringCallback(): Promise<(value: string) => string>;
  compareStrings(left: string, right: string): boolean;
}

/** @throws {TypeError} */
export declare function parseString(value: string): string;

export declare function roundtripBoolean(value: boolean): boolean;
export declare function roundtripNumber(value: number): number;
