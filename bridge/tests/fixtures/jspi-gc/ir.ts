ir 1;

class Api {
  later(): future<u32>;
  mark(): unit;
}

export namespace app {
  run(api: Api): future<u32>;
  echo(api: Api, s: str): future<u32>;
  race(api: Api): future<u32>;
  stuck(api: Api): u32;
  fail(api: Api, msg: Str): future<u32> throws err;
}
