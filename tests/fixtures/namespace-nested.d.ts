//! @ts-gen --lib-name namespace-nested --export namespace-nested --export tests/fixtures/namespace-nested.d.ts

// A nested namespace binds its members to their full JS path, so
// `Outer.Inner.Kind` and `Other.Inner.Kind` stay apart even though both
// end in `Inner.Kind`.

declare global {
  namespace Outer {
    namespace Inner {
      class Thing {
        static make(): Thing;
      }
      function run(): void;
      const LIMIT: number;
      type Kind = "a" | "b";
      enum Level {
        One = 1,
      }
    }
  }

  namespace Other {
    namespace Inner {
      type Kind = "x" | "y";
    }
  }
}
