// Type-level generics on class-like declarations: every parameter gets a
// Rust default (the TS default, else `JsValue`) so the bare name stays
// usable; constructors / statics only bind the parameters they mention;
// subclasses of generic parents extend the bare head; every type derives
// `Eq`, generic ones and children of generic parents included. An async
// static or free function resolving to a bare type parameter bounds it
// `FromWasmAbi` too. A constructor fills an unmentioned parameter whose
// default names an earlier unmentioned one (`B = A`) with that one's
// argument, never the bare name. A `keyof` parameter that types another
// argument has a `_typed` twin taking the key as a marker type.

interface Event {
  readonly type: string;
}

interface MessageEvent extends Event {
  readonly data: string;
}

type EventListener<E extends Event = Event> = (event: E) => void;

interface EventListenerObject<E extends Event = Event> {
  handleEvent(event: E): void;
}

type EventListenerOrEventListenerObject<E extends Event = Event> =
  | EventListener<E>
  | EventListenerObject<E>;

declare class EventTarget<EventMap extends Record<string, Event> = Record<string, Event>> {
  constructor();
  addEventListener<Type extends keyof EventMap>(
    type: Type,
    handler: EventListenerOrEventListenerObject<EventMap[Type]>,
  ): void;
  dispatchEvent(event: Event): boolean;
}

interface SocketEventMap {
  message: MessageEvent;
  close: Event;
}

declare class Socket extends EventTarget<SocketEventMap> {
  // A keyed return stays on the erased reading.
  last<Type extends keyof SocketEventMap>(type: Type): SocketEventMap[Type];
}

declare function addEventListener<Type extends keyof SocketEventMap>(
  type: Type,
  handler: EventListener<SocketEventMap[Type]>,
): void;

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
  static load<T>(): Promise<T>;
  readonly value: T;
}

declare class Chained<A = string, B = A, C = number> {
  constructor(c: C);
}

interface Storage<Props = unknown> {
  get<T = unknown>(key: string): Promise<T | undefined>;
  run<T>(callback: () => Promise<T>): Promise<T>;
  props(): Props;
}

declare function loadValue<T>(): Promise<T>;
