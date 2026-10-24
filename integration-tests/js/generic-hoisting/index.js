export class Envelope {
  constructor(label) {
    this.label = label;
  }
  forward(other) {
    return `${this.label}->${other.label}`;
  }
  copy() {
    return new Envelope(this.label);
  }
}

export class Courier {
  constructor(first) {
    this.first = first;
  }
  send(envelope) {
    return `send:${this.first.label}:${envelope.label}`;
  }
  sendLabelled(envelope) {
    return `labelled:${envelope.label}`;
  }
  async sendAsync(envelope) {
    return `async:${envelope.label}`;
  }
  sendEither(envelope) {
    return typeof envelope === "string" ? `str:${envelope}` : `env:${envelope.label}`;
  }
  sendMaybe(envelope, tag) {
    return `${tag}:${envelope === undefined ? "none" : envelope.label}`;
  }
  static check(envelope) {
    return `check:${envelope.label}`;
  }
}

export const post = (envelope) => `post:${envelope.label}`;
export const postAsync = async (envelope) => `post-async:${envelope.label}`;
export const relay = (envelope, meta) => `relay:${envelope.label}:${meta}`;
