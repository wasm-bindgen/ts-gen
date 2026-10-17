//! @ts-gen --lib-name enum-initializers --export enum-initializers

// A member initialiser can reference another enum's members, including one
// inside a namespace, and the members of every declaration of a merged enum.

export declare enum Base {
  Default = 2,
  Other = 4,
}

export declare enum Later {
  X = 16,
}

export declare enum Mode {
  A = Base.Default,
  B,
  C = A | 8,
  D = Later.X + Mode.B,
  E = Base["Other"],
}

export declare namespace Ns {
  enum Inner {
    Value = "inner",
  }
}

export declare enum Label {
  Inner = Ns.Inner.Value,
  Joined = Ns.Inner.Value + "-label",
}

export declare enum Merged {
  A = 1,
}

export declare enum Merged {
  B = A + 1,
  C,
}

// Each declaration's first member counts from 0 without an initialiser.
export declare enum Restart {
  A = 5,
}

export declare enum Restart {
  B,
  C,
}

// A cycle has no constant value.
export declare enum Cycle {
  A = Loop.B,
  Fixed = 1,
}

export declare enum Loop {
  B = Cycle.A,
  C = Cycle.Fixed,
}

export declare function useAll(mode: Mode, label: Label, merged: Merged, restart: Restart, cycle: Cycle, loop: Loop): void;
