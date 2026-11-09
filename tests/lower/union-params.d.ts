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
