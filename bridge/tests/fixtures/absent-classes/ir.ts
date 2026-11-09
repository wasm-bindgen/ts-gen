ir 1;

// Only `Present` is a JS global: `Absent`, `ns` and the module's `Gone`
// have no value at runtime.
class Present {}
class Absent {}
namespace ns {
  class Deep {}
}

enum Pick {
  A(Absent);
  P(Present);
  N(f64);
  @fallback Other(Any);
}
enum Only {
  A(Absent);
}

absent(): Absent;
maybe(): Absent?;
deep(): ns.Deep;
pick(): Pick;
only(): Only;
present(): Present;
export take(a: Absent): u32;

module "./host.mjs" {
  class Gone {}
  gone(): Gone;
  export { Gone, gone };
}
