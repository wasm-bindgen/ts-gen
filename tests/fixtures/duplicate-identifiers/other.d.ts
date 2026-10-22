// Declarations of globals `duplicate-identifiers.d.ts` declares first.

interface Dup {
  c: boolean;
}

interface IfaceFirst {
  c: boolean;
}

declare class TwoClasses {
  c(): void;
}

type Merged = { c: boolean };

declare namespace N {
  interface T {
    c: boolean;
  }
}

declare function useOther(
  dup: Dup,
  first: IfaceFirst,
  two: TwoClasses,
  merged: Merged,
  t: N.T,
): void;
