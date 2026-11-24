//! @ts-gen --export Bucket --export openBucket --export listBuckets --export Feed --export cloudflare:sockets

// Root selection: `Bucket`, `openBucket` and `listBuckets` are roots; the
// closure reaches `BucketObject`, `GetOptions`, `Checksums` (through a method
// return / parameter / nested property), `Conditional` (through the
// options bag), `Socket` (through the exported module), the
// `BucketKind` enum, and `ListDefaults` (through a later overload's own
// type-parameter default). `Unrelated` and `Dangling` are never reached.
// `Feed` reaches `Listeners` and its map `FeedEvents`, so its keyed `on`
// has a `_typed` twin and a key for each of the map's values; `Hidden` and
// its map `HiddenEvents` are not reached, and neither is named by a twin
// or a key.

interface Checksums {
  md5?: string;
}

interface Conditional {
  etagMatches?: string;
}

interface GetOptions {
  onlyIf?: Conditional;
}

interface BucketObject {
  readonly key: string;
  readonly checksums: Checksums;
}

type BucketKind = "standard" | "infrequent";

declare abstract class Bucket {
  get(key: string, options?: GetOptions): Promise<BucketObject | null>;
  kind(): BucketKind;
  setKind(kind: BucketKind): void;
}

declare function openBucket(name: string): Bucket;

interface ListDefaults {
  limit?: number;
}

declare function listBuckets(prefix: string): string[];
// No argument infers `T`, so the binding returns it at this default.
declare function listBuckets<T = ListDefaults>(): Map<string, T>;

interface Unrelated {
  nothing: number;
}

interface Dangling {
  bucket: Bucket;
}

interface Socket {
  readonly closed: Promise<void>;
}

interface SocketOptions {
  secureTransport?: "off" | "on";
}

declare module "cloudflare:sockets" {
  function connect(address: string, options?: SocketOptions): Socket;
}

interface Notice {
  readonly text: string;
}

interface Secret {
  readonly code: number;
}

interface NoticeListener<E = Notice> {
  (event: E): void;
}

declare class Listeners<M extends Record<string, object> = Record<string, object>> {
  on<K extends keyof M>(type: K, handler: NoticeListener<M[K]>): void;
}

declare namespace Feeds {
  interface Tick {
    readonly count: number;
  }
}

// A builtin and a namespace's member are values the roots can't leave out.
interface FeedEvents {
  notice: Notice;
  opened: Date;
  tick: Feeds.Tick;
}

declare class Feed extends Listeners<FeedEvents> {
  latest(): Notice;
}

interface HiddenEvents {
  secret: Secret;
}

declare class Hidden extends Listeners<HiddenEvents> {}
