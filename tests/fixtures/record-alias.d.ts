//! @ts-gen --experimental-generic-mono

export type EvaluationContext = Record<string, string | number | boolean>;
export type Labels = Record<string, string>;
export type Values<T> = Record<string, T>;
export type MixedValues = Record<string, string | string[]>;
export type FlexibleRecord = Record<string | number, string | boolean>;
export type KnownLabels = Record<"displayName" | "region", string>;
export type KnownValues = Record<"name" | "enabled", string | boolean>;

export declare function evaluate(context: EvaluationContext): void;
export declare function labelAll(labels: Labels): void;
