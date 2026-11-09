//! @ts-gen --provides tests/fixtures/side/export-class.d.ts

// `export declare class` in a `--provides` file is an `export class`
// (§17): its members are exports, an instance is the language's handle,
// and it may extend one imported class. A setter or parameter of `T | null`
// takes `T?`, since an export receives from JS. The language constructs a
// base-less class natively, so `Registry` declares no constructor.

interface Env {}
declare class Request {}
declare class Response {}
interface DurableObjectState {
  readonly id: string;
}
declare abstract class DurableObject {
  constructor(ctx: DurableObjectState, env: Env);
  fetch?(request: Request): Response | Promise<Response>;
}
