ir 1;

class Api {
  later(): future<u32>;
}

export namespace app {
  run(api: Api): future<u32>;
  echo(api: Api, s: str): future<u32>;
  share(api: Api): u32;
  peek(): future<u32>;
  unshare(): u32;
  race(api: Api): future<u32>;
  lowest(api: Api): future<u32>;
  empty(): future<u32>;
  plus(n: u32): fn(Api) -> future<u32>;
}

export class Task {
  new(api: Api);
  run(): future<u32>;
}
