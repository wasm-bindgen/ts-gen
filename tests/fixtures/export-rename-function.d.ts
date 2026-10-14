//! @ts-gen --export tests/fixtures/export-rename-function.d.ts --export node:thing

// `export { local as public }` publishes a function under the public name:
// the binding's js_name is `connect`, the only name the module exports.
declare module "node:thing" {
  function _connect(address: string): Socket;
  export { _connect as connect };
}

interface Socket {
  close(): Promise<void>;
}
