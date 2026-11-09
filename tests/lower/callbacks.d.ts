//! @ts-gen --lib-name callbacks --once Thenable.then.onfulfilled --once queueMicrotask.callback

// Callbacks: function types are `fn(...)`, and config marks the
// ones called at most once `@once`.

interface Thenable {
  then(onfulfilled: (value: string) => void, onrejected?: (reason: any) => void): void;
}
declare function queueMicrotask(callback: () => void): void;
declare function on(listener: ((e: string) => number) | null): void;

// A named function type is an alias, a recursive one too.
type Listener = (e: string) => void;
type Next<T> = (next: Next<T>, value: T) => T;
declare function listen(l: Listener, again: Listener | null): void;
declare function step(n: Next<number>): void;
