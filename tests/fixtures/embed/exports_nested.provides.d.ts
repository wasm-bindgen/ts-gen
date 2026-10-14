// A module namespace beyond the default object: a named function, a
// nested object, and a default implementing the worker interface.
export function version(): number;
export const api: {
  math: {
    add(a: number, b: number): number;
  };
  describe: (req: Request) => string;
};
declare const handler: {
  fetch(request: Request, env: Env, ctx: ExecutionContext): Promise<Response>;
};
export default handler;
