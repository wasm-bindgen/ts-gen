export function fail(message) {
  throw new Error(message);
}

export function failRange(n) {
  throw new RangeError(`out of range: ${n}`);
}

export function failAnyway() {
  throw new Error("anyway");
}

export function rejectLater(message) {
  return Promise.reject(new Error(message));
}

export function check(value) {
  if (typeof value === "number") throw new TypeError(`not a string: ${value}`);
  return value;
}

export function raise(kind, message) {
  throw kind === "range" ? new RangeError(message) : new TypeError(message);
}

export function read(kind) {
  if (kind === "fail") throw new Error("read failed");
  return kind;
}

export const abort = (code) => {
  throw new Error(`abort ${code}`);
};

export class Guard {
  valueOf() {
    throw new TypeError("a guard has no primitive value");
  }
  failLater() {
    return Promise.reject(new Error("later"));
  }
  reject(kind) {
    throw new Error(`rejected ${kind}`);
  }
  static unreachable(reason) {
    throw new Error(`unreachable: ${reason}`);
  }
}

export class Halt {
  ok() {
    return true;
  }
  static stop(code) {
    throw new Error(`stop ${code}`);
  }
}
