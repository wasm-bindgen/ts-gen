// The worker. The generated entry (workers.provided.fetch, the module's
// `fetch` export) wraps the wire values and calls this function inside
// `promise {}`, so it may suspend: `await` on the `Promise` that `text()`
// returns. TS strings are Kotlin `String`s; `Response(...)` and
// `ResponseInit()` are the owners' constructors.
@file:OptIn(kotlin.js.ExperimentalWasmJsInterop::class)
package app

import kotlinx.coroutines.await
import workers.Env
import workers.ExecutionContext
import workers.Headers
import workers.Request
import workers.Response
import workers.ResponseInit

suspend fun fetch(request: Request, env: Env, ctx: ExecutionContext): Response {
    val header = request.headers.get("x-kotlin")
    val body = request.text().await<JsAny?>()?.unsafeCast<JsString>()?.toString() ?: ""
    val reply = "${request.method} ${request.url}\n" +
        "x-kotlin: ${header ?: "(none)"}\n" +
        "GREETING: ${env.GREETING}\n" +
        "body: $body\n"
    val headers = Headers()
    headers.set("content-type", "text/plain; charset=utf-8")
    val init = ResponseInit()
    init.setStatus(200.0)
    init.setHeaders(headers.ref)
    return Response(reply.toJsString(), init)
}
