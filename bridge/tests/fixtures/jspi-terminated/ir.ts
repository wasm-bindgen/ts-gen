ir 1;

class Api {
  later(): future<u32>;
}

export namespace app {
  run(api: Api): future<u32>;
}
