//! @ts-gen --experimental-generic-mono

export type EvaluationContext = Record<string, string | number | boolean>;
export type Labels = Record<string, string>;
export type Values<T> = Record<string, T>;
export type MixedValues = Record<string, string | string[]>;

export declare function evaluate(context: EvaluationContext): void;
export declare function labelAll(labels: Labels): void;
