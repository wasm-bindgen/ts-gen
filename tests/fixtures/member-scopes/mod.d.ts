import { T as Ev } from "ia";

declare global {
  interface Win {
    b: Ev;
    m(): Ev;
  }
  namespace GN {
    function gm(e: Ev): void;
  }
  interface XCtor {
    new (e: Ev, n: number): X;
    sm(e: Ev): void;
  }
}
