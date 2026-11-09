//! @ts-gen --lib-name exports --provides Handler.scheduled

// Exports: `--provides Handler.scheduled` exports the default
// object's required members, and the selected optional one, as an
// `export namespace default`.

interface Env {}
declare class Request {}
declare class Response {}
interface Handler {
  fetch(request: Request, env: Env): Response | Promise<Response>;
  scheduled?(time: number): void;
  queue?(batch: string[]): void;
}
