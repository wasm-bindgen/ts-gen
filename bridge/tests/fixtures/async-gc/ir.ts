ir 1;

class Api {
  fetch(): future<str>;
  futures(): arr<Any>;
}

export namespace app {
  run(): future<str>;
}
