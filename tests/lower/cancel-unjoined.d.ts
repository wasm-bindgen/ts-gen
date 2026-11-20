//! @ts-gen --lib-name cancel_unjoined

// A cancel signal joins the caller's through `AbortSignal.any`. An input
// that declares the controller without it has no cancellable member, and
// says so once.

declare class AbortSignal {
  readonly aborted: boolean;
}
declare class AbortController {
  constructor();
  readonly signal: AbortSignal;
  abort(reason?: any): void;
}
interface RequestInit {
  method?: string;
  signal?: AbortSignal | null;
}
declare function fetch(input: string, init?: RequestInit): Promise<string>;
declare function send(body: string, init?: RequestInit): Promise<void>;
declare function ping(): Promise<void>;
