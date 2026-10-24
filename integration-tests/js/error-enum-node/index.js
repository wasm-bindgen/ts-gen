export class Fault extends Error {
  constructor(message) {
    super(message);
    this.name = "Fault";
  }
}

export class SubFault extends Fault {
  constructor(message) {
    super(message);
    this.name = "SubFault";
  }
}

export class Other extends Error {}

export class CodesFault extends Error {}

export const Codes = {
  Fault: class Fault extends Error {},
};

function thrown(kind) {
  switch (kind) {
    case "sub":
      return new SubFault("sub fault");
    case "fault":
      return new Fault("plain fault");
    case "type":
      return new TypeError("bad type");
    case "string":
      return "just a string";
    case "number":
      return 42;
    case "opaque":
      return Object.create(null);
    case "other":
      return new Other("declared other");
    case "codes-fault":
      return new CodesFault("flat fault");
    case "codes.fault":
      return new Codes.Fault("namespaced fault");
  }
  return undefined;
}

export function raise(kind) {
  const value = thrown(kind);
  if (value !== undefined) throw value;
  return kind;
}

export function raiseNamed(kind) {
  return raise(kind);
}

export function raiseUntyped(kind) {
  return raise(kind);
}

export class Gate {
  constructor(open) {
    this.open = open;
  }
  get value() {
    if (!this.open) throw new SubFault("gate closed");
    return 1;
  }
}
