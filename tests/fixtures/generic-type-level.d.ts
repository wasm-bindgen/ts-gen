// Type-level generics on class-like declarations: every parameter gets a
// Rust default (the TS default, else `JsValue`) so the bare name stays
// usable; constructors / statics only bind the parameters they mention;
// subclasses of generic parents extend the bare head; every type derives
// `Eq`, generic ones and children of generic parents included.

interface Event {
  readonly type: string;
}

declare class EventTarget<EventMap extends Record<string, Event> = Record<string, Event>> {
  constructor();
  dispatchEvent(event: Event): boolean;
}

declare class AbortSignal extends EventTarget {
  readonly aborted: boolean;
}

declare class PingTarget extends EventTarget<{ ping: Event }> {}

interface Bag extends Record<string, unknown> {
  readonly size: number;
}

interface Transformer<I = any, O = any> {
  readableType?: I;
  writableType?: O;
}

declare class TransformStream<I = any, O = any> {
  constructor(transformer?: Transformer<I, O>);
  readonly locked: boolean;
}

declare class Labelled<T> {
  constructor(value: T);
  static empty(): Labelled;
  static of<T>(value: T): Labelled<T>;
  readonly value: T;
}

interface Storage<Props = unknown> {
  get<T = unknown>(key: string): Promise<T | undefined>;
  run<T>(callback: () => Promise<T>): Promise<T>;
  props(): Props;
}
