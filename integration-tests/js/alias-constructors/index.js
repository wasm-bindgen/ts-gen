export class Wrap {
  #value;
  constructor(value) {
    this.#value = value;
  }
  get() {
    return this.#value;
  }
}

export const Wrap2 = Wrap;

export function unwrapText(stub) {
  return stub.get();
}

export class Label {
  #text;
  constructor(text, unit) {
    this.#text = unit === undefined ? text : `${text}${unit}`;
  }
  text() {
    return this.#text;
  }
}

export class Pt {
  x = 1;
}

export const ProtoOnly = {};

export class WithStatics extends Wrap {
  static helper() {
    return 1;
  }
}

export class StaticNew {
  static new() {
    return new StaticNew();
  }
  y() {}
}

export const SN = StaticNew;

export class CN {
  New() {}
}

export class Count {
  #n;
  constructor(start) {
    this.#n = start;
  }
  count() {
    return this.#n;
  }
}

export class Multi {
  #n;
  constructor(n) {
    this.#n = n;
  }
  a() {
    return "a";
  }
  b() {
    return this.#n;
  }
}
