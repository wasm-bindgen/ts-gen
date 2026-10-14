//! @ts-gen --export tests/fixtures/export-rename-function.d.ts --export node:thing --export node:other

// `export { local as public }` binds a value to its public names, the only
// names the module exports.
declare module "node:thing" {
  function _connect(address: string): Socket;
  function _listen(port: number): void;
  function _close(): void;
  const _version: string;
  class _Server {
    constructor(port: number);
    stop(): void;
  }
  export { _connect as connect, _listen as serve, _listen as listenOn };
  export { _close as shutdown, _version as version, _Server as Server };
}

// The export list is scoped to its module: the same local names elsewhere
// keep their own bindings.
declare function _listen(host: string): void;

declare module "node:other" {
  export function _close(): boolean;
}

interface Socket {
  close(): Promise<void>;
}
