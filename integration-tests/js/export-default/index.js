export default class Processor {
  constructor(name) {
    this.name = name;
  }
  static named(name) {
    return new Processor(`named ${name}`);
  }
}
