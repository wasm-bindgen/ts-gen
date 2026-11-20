// The second service: the target of the worker's subrequests, the
// backing of its Cache API, and the caller of its other handlers.
const notes = [];
const cached = new Map();

export default {
  async fetch(request, env) {
    const url = new URL(request.url);
    // The Cache API's protocol: `put` sends the response as an HTTP message.
    if (request.headers.get('cache-control') === 'only-if-cached' || request.method === 'PUT' || request.method === 'PURGE') {
      if (request.method === 'PUT') {
        cached.set(request.url, await request.text());
        return new Response(null, { status: 204 });
      }
      if (request.method === 'PURGE') {
        return new Response(null, { status: cached.delete(request.url) ? 200 : 404 });
      }
      const message = cached.get(request.url);
      if (message === undefined) return new Response(null, { status: 504 });
      const [head, body] = message.split('\r\n\r\n');
      const headers = head.split('\r\n').slice(1).map((line) => line.split(': '));
      return new Response(body, { headers: [...headers, ['cf-cache-status', 'HIT']] });
    }
    switch (url.pathname) {
      case '/note':
        notes.push(await request.text());
        return new Response('noted\n');
      case '/notes':
        return new Response(`helper's notes: ${notes.join('; ') || '(none)'}\n`);
      case '/catch': {
        // Calls the worker's throwing routes, and says what JS caught.
        let out = '';
        for (const path of ['/fail/typed', '/fail/plain']) {
          try {
            await env.MAIN.fetch(`http://main${path}`);
            out += `${path} threw nothing\n`;
          } catch (e) {
            out += `${path} rejected with a JS ${e.name}: ${e.message}\n`;
          }
        }
        return new Response(out);
      }
      case '/scheduled': {
        // Runs the worker's scheduled handler, as a cron trigger would.
        const { outcome } = await env.MAIN.scheduled({ cron: '*/5 * * * *' });
        return new Response(`scheduled: ${outcome}; ${notes.at(-1)}\n`);
      }
      case '/queue': {
        // Delivers a batch to the worker's queue handler, as a queue would.
        const messages = ['first', 'second'].map((body, i) => ({ id: `${i}`, timestamp: new Date(), body, attempts: 1 }));
        const { outcome } = await env.MAIN.queue('tour', messages);
        return new Response(`queue: ${outcome}; ${notes.at(-1)}\n`);
      }
      case '/slow':
        await scheduler.wait(5000);
        return new Response('slow\n');
      default:
        return new Response(`helper saw ${request.method} ${url.pathname} "${await request.text()}"\n`);
    }
  },
};
