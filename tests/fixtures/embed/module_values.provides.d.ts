// Hands back the scalar exports of "embed:values" as the module read them,
// and what `receiver` saw as `this`. A boolean result reaches JS as a
// boolean, not the `i32` the module returns.
export function count(): number;
export function enabled(): boolean;
export function disabled(): boolean;
export function big(): bigint;
export function receiver(): string;
