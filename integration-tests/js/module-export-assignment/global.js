export function greet(name) {
  return `hello ${name}`;
}
export class Greeter {
  constructor(prefix) {
    this.prefix = prefix;
  }
  greet(name) {
    return `${this.prefix} ${name}`;
  }
}
