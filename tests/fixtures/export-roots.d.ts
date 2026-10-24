//! @ts-gen --export Bucket --export openBucket --export listBuckets --export cloudflare:sockets

// Root selection: `Bucket`, `openBucket` and `listBuckets` are roots; the
// closure reaches `BucketObject`, `GetOptions`, `Checksums` (through a method
// return / parameter / nested property), `Conditional` (through the
// options bag), `Socket` (through the exported module), the
// `BucketKind` enum, and `ListDefaults` (through a later overload's own
// type-parameter default). `Unrelated` and `Dangling` are never reached.

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
