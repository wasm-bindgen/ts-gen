//! @ts-gen --export tests/fixtures/split-class.d.ts --export cloudflare:email --export msg:a --export msg:b --export late

// Upstream splits a class into a global `interface X` (the instance
// shape) and a module-scoped constructor variable exported as `X`. The
// two collapse into one class: the interface's members move onto the
// class and every reference to `X` resolves to it.

interface EmailMessage {
  readonly from: string;
  readonly to: string;
}

interface ForwardableEmailMessage extends EmailMessage {
  reply(message: EmailMessage): Promise<void>;
}

declare module "cloudflare:email" {
  let _EmailMessage: {
    prototype: EmailMessage;
    new (from: string, to: string, raw: string): EmailMessage;
  };
  export { _EmailMessage as EmailMessage };
}

// The construct signature's `Msg` is the global interface, which `msg:b`'s
// own `Msg` shadows there and keeps.
interface Msg {
  readonly from: string;
}

declare module "msg:a" {
  let _Msg: { new (from: string): Msg };
  export { _Msg as Msg };
}

declare module "msg:b" {
  interface Msg {
    other: number;
  }
  export function use(m: Msg): void;
}

// The interface may follow the module that constructs it.
declare module "late" {
  let _Late: { new (): Late };
  export { _Late as Late };
}

interface Late {
  readonly at: number;
}

declare function arrival(): Late;
