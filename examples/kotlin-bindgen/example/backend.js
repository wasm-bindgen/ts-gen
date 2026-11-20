// The second service of config.capnp, in plain JS: what `/fetch` reaches
// through the BACKEND binding and, as the worker's outbound, through the
// global `fetch`. `/slow` takes two seconds, and `/aborts` says how many
// of its requests were aborted since it was last asked.
let aborts = 0;

export default {
  async fetch(request) {
    const { pathname } = new URL(request.url);
    if (pathname === '/slow') {
      request.signal.addEventListener('abort', () => { aborts++; });
      await scheduler.wait(2000);
    }
    if (pathname === '/aborts') {
      const seen = aborts;
      aborts = 0;
      return new Response(String(seen));
    }
    const body = await request.text();
    return new Response(`${request.method} ${pathname}${body && `, with "${body}"`}, from the backend`);
  },
};
