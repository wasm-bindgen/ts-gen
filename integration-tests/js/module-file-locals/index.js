class _Foo {
  bar() {
    return "bar";
  }
}
export { _Foo as Foo };

class _Server {
  listen() {
    return "listening";
  }
}
export { _Server as Server };

export function configure(o) {
  return new _Foo();
}

function _ren() {
  return "ren";
}
export { _ren as ren };

// Module-private: a class the module returns but never exports, and a
// helper and constant with no export at all.
class Hidden {
  static make() {
    return new Hidden();
  }
  m() {
    return "m";
  }
}
export function reveal() {
  return Hidden.make();
}
function helper() {}
const secret = 42;
