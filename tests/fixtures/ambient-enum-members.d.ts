//! @ts-gen --lib-name ambient-enum-members --export ambient-enum-members

// Every enum in a .d.ts is ambient, and TypeScript gives a member of an
// ambient non-const enum without an initialiser no value: it's computed, not
// auto-incremented. Such a member is skipped. It's still a number, so an
// enum left with only such members is erased to number.
export declare enum Computed {
  A,
  B,
}

// Initialised members keep their values; the uninitialised one is skipped.
export declare enum Mixed {
  A = 1,
  B,
  C = 4,
}

// A const enum's members are numbered: 0 first, then one past a numeric
// previous member. One after a string member has no value.
export declare const enum Numbered {
  A,
  B,
  C = 10,
  D,
}

export declare const enum AfterString {
  A = "a",
  B,
}

export declare function useAll(
  computed: Computed,
  mixed: Mixed,
  numbered: Numbered,
  afterString: AfterString,
): void;
