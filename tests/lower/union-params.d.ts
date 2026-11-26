//! @ts-gen --provides Handler

// A union parameter is one `union` payload enum, named for a named union,
// also beside `null`, and for its arms otherwise. A union the bridge can't
// tell apart is only a named one's enum, and an anonymous one expands into
// overloads, as an export's union always does where it can't.

declare class Blob {}
declare class Stream {}
interface Chunk {
  size: number;
}
interface Part {
  name: string;
}
type Body = string | Blob | Stream;
type Piece = Chunk | Part;
declare function send(body: Body): void;
declare function sendMaybe(body?: Body | null): void;
declare function sendAny(body: string | Blob): void;
declare function sendPiece(piece: Piece): void;

// An alias of an alias, and a union the alias already holds, are the
// alias. A generic alias is named for the arguments its union reads.
type Payload = Body;
type Wrapped<T = Chunk> = T | Part;
type Tagged<M = unknown> = Blob | Stream;
declare function sendPayload(body: Payload): void;
declare function sendLoose(body: Body | string): void;
declare function wrapDefault(w: Wrapped): void;
declare function wrapBlob(w: Wrapped<Blob>): void;
declare function tagged(t: Tagged<string>): void;
interface Handler {
  receive(body: Body): void;
  take(piece: Piece): void;
}

// `Exclude` filters a union alias's values, and passes an object whole.
type Value = string | number | string[];
declare function scalar(v: Exclude<Value, string[]> | null): void;
interface Settings { id: string; cache?: boolean }
type Universal = Exclude<Settings, "id"> & { extra?: string };
declare function configure(s: Universal): void;

// `ArrayBuffer` beside `ArrayBufferView` is a `buffer`, whatever else the
// union holds and however it's named. As a type argument the two stay an
// enum. `{@slice view}` writes it by arm, beside its view.
type BufferSource = ArrayBufferView | ArrayBuffer;
declare class Box<T> {
  value: T;
}
declare function digest(data: BufferSource): ArrayBuffer;
declare function store(key: string, value: string | ArrayBuffer | ArrayBufferView): void;
declare function storeBlob(value: Blob | BufferSource | string): void;
declare function load(): ArrayBuffer | ArrayBufferView | null;
declare function loadText(): BufferSource | string;
declare function join(parts: BufferSource[]): Promise<BufferSource>;
declare function boxed(): Box<BufferSource>;
/** @param data {@slice view} */
declare function hash(data: BufferSource): void;
/** @param data {@slice view} */
declare function hashText(data: string | BufferSource): void;
interface Frame {
  payload: ArrayBuffer | ArrayBufferView;
  tag?: ArrayBufferView | ArrayBuffer | string;
}
