// Per-monomorphization, `M[K]` over a concrete `M` expands into one binding
// per key: `K` is the key's literal and `M[K]` its property type, reached
// through `extends EventTarget<SocketEventMap>` or declared directly, with
// the other parameters expanding as in the generic form (`_with_options`,
// `_with_bool`). A lone literal arm keys its binding where another overload
// takes something else at its position (`on_alpha`, `on_beta`).

interface Ev {
  type: string;
}
interface MessageEv extends Ev {
  data: string;
}
interface CloseEv extends Ev {
  code: number;
}
type Listener<E extends Ev = Ev> = (event: E) => void;
interface ListenerObject<E extends Ev = Ev> {
  handleEvent(event: E): void;
}
type ListenerOrObject<E extends Ev = Ev> = Listener<E> | ListenerObject<E>;
interface ListenerOptions {
  once?: boolean;
}
declare class Target<EventMap extends Record<string, Ev> = Record<string, Ev>> {
  addEventListener<Type extends keyof EventMap>(
    type: Type,
    handler: ListenerOrObject<EventMap[Type]>,
    options?: ListenerOptions | boolean,
  ): void;
}
type SocketEventMap = {
  close: CloseEv;
  message: MessageEv;
};
declare class Socket extends Target<SocketEventMap> {
  send(data: string): void;
}
declare function addEventListener<Type extends keyof SocketEventMap>(
  type: Type,
  handler: Listener<SocketEventMap[Type]>,
): void;

declare class Emitter {
  on(type: "alpha", handler: (event: MessageEv) => void): void;
  on(type: "beta", handler: (event: CloseEv) => void, options?: ListenerOptions): void;
}
