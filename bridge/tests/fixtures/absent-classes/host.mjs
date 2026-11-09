// `Gone` is declared exported but isn't.
let next;
export function setNext(v) {
  next = v;
}
export function gone() {
  return next;
}
