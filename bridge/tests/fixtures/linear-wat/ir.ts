ir 1;

class Target {
  on(cb: fn(u32) -> u32);
  fire(x: u32): u32;
}

export namespace app {
  listen(t: Target, base: u32);
  forget();
  guarded(t: Target, s: str): u32;
  terminate();
}
