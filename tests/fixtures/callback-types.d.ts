// Fixture: function types lower to wasm-bindgen's typed `Function<fn(…) ->
// R>` with inside-generic element lowering. A rest parameter has no fixed
// arity to spell, and an all-`any` signature carries no information, so
// both lower to the untyped `Function` that any closure or JS function
// (a `Promise` executor's `resolve`) satisfies directly.

declare class Timer {
  setTimeout(callback: (...args: any[]) => void, msDelay?: number): number;
  onTick(callback: (tick: number, label: string) => boolean): void;
  onAny(callback: (value: any) => any): void;
  onRest(callback: (first: string, ...rest: number[]) => void): void;
}
