export function checkedSqrt(n) {
  if (n < 0) throw new Error(`negative: ${n}`);
  return Math.sqrt(n);
}

export function double(n) {
  return n * 2;
}

export function clampIndex(n, len) {
  if (n < 0 || n >= len) throw new RangeError(`index ${n} out of range`);
  return n;
}

export function parseLater(s) {
  return Promise.resolve(Number(s));
}

export class Counter {
  constructor(start) {
    this.value = start;
    this.max = 10;
  }
  increment(by = 1) {
    if (this.value + by > this.max) throw new Error("overflow");
    this.value += by;
    return this.value;
  }
}
