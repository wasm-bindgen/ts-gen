export default class Counter {
  constructor(start) {
    this.value = start;
  }
  increment() {
    this.value += 1;
  }
}
