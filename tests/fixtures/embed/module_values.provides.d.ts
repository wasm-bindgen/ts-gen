// Hands back the scalar exports of "embed:values" as the module read them,
// a boolean as the number it reads as, and what `receiver` saw as `this`.
export function count(): number;
export function enabled(): number;
export function disabled(): number;
export function big(): bigint;
export function receiver(): string;
