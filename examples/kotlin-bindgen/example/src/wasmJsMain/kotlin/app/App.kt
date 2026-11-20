// The worker's entry: the default export's `fetch`, a suspend function the
// runtime runs as a task. It routes to one function per route, grouped by
// topic in the files beside this one.
package app

import workers.*

@Suppress("MayBeConstant")
private val INDEX = """
    A tour of the Workers API from Kotlin/Wasm. Routes:
      /echo        the request read back: method, URL, headers, body
      /url         URL and URLSearchParams
      /form        FormData fields: strings and files
      /json        JSON in and out
      /crypto      digest, random values, text encoding, base64
      /stream      ReadableStream, TransformStream, a streamed body
      /fetch       subrequests: a service binding, fetch, cancellation
      /timers      scheduler.wait, setTimeout, queueMicrotask, waitUntil
      /rewrite     HTMLRewriter
      /counter/*   a Durable Object: increment, list, reset, sql, alarm
      /ws          a WebSocket echo (/counter/ws hibernates)
      /rpc         an RPC entrypoint called through its stub
      /errors      JS errors caught as typed exceptions
      /fail        a typed exception thrown to JS (/fail/plain: any other)
      /scheduled   runs the scheduled handler
      /queue       runs the queue handler
""".trimIndent() + "\n"

suspend fun fetch(request: HttpRequest, env: Env, ctx: ExecutionContext<JsAny?>): Response {
    val url = URL(request.url)
    return when (val path = url.pathname) {
        "/" -> text(INDEX)
        "/echo" -> echo(request)
        "/url" -> url(url)
        "/form" -> form(request)
        "/json" -> json(request)
        "/crypto" -> crypto()
        "/stream" -> stream()
        "/fetch" -> subrequests(env)
        "/timers" -> timers(ctx)
        "/rewrite" -> rewrite()
        "/counter/alarm" -> alarm(env)
        "/ws" -> webSocket()
        "/rpc" -> rpc(env)
        "/errors" -> errors(env)
        "/fail", "/fail/plain" -> fail(path)
        "/scheduled" -> runScheduled(env)
        "/queue" -> runQueue(env)
        // The rest of /counter is the Durable Object's own: its stub is a Fetcher.
        else -> if (path.startsWith("/counter/")) counter(env).fetch(request).await() else text("no route for $path\n", 404u)
    }
}
