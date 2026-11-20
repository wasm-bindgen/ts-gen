ir 1;

enum FetchError extends err {
  @js(DOMException[name = "AbortError"]) Aborted(Str);
}

class AbortController {
  new();
  get signal(): AbortSignal;
  abort(): unit;
}

class AbortSignal {}

class Api {
  fetch(url: str): future<str, FetchError>;
  @js(let c = AbortController.new(), args = (url, c.signal), cancel = c.abort)
  slow(url: str): future<u32>;
  number(): future<u32>;
  nothing(): future<unit>;
  forever(): future<never>;
  strict(): future<u32> throws;
  give(f: future<u32>);
  later(cb: fn(u32) -> future<u32>);
}

export namespace app {
  run(x: u32): future<u32>;
  wait(p: future<str>): u32;
}
