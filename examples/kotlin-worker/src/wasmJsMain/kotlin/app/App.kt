// The worker. The generated entry (workers.provided.fetch, the module's
// `fetch` export) calls this function inside `promise {}`, so it may
// suspend: `await` on the `Promise<JsString>` that `text()` returns. Classes
// are external interfaces with extension members; `Headers()`,
// `ResponseInit()` and `Response(...)` are their constructors, `BodyInit(...)`
// makes the union `Response` takes from a string, and a union read back is
// matched on its `arm`.
@file:OptIn(kotlin.js.ExperimentalWasmJsInterop::class)
package app

import workers.BodyInit
import workers.Env
import workers.ExecutionContext
import workers.greeting
import workers.await
import workers.Headers
import workers.HeadersKindArm
import workers.arm
import workers.Request
import workers.Response
import workers.ResponseInit
import workers.get
import workers.headers
import workers.method
import workers.set
import workers.setHeaders
import workers.setStatus
import workers.text
import workers.url

suspend fun fetch(request: Request, env: Env, ctx: ExecutionContext): Response {
    val header = request.headers.get("x-kotlin")
    val body = request.text().await().toString()
    val reply = "${request.method} ${request.url}\n" +
        "x-kotlin: ${header ?: "(none)"}\n" +
        "GREETING: ${env.greeting}\n" +
        "body: $body\n"
    val headers = Headers()
    headers.set("content-type", "text/plain; charset=utf-8")
    val init = ResponseInit()
    init.setStatus(200.0)
    init.setHeaders(headers)
    val set = when (val arm = init.headers?.arm) {
        is HeadersKindArm.Headers -> "Headers(content-type: ${arm.value.get("content-type")})"
        is HeadersKindArm.VecOfTuple -> "pairs"
        is HeadersKindArm.Record -> "record"
        null -> "(none)"
    }
    return Response(BodyInit(reply + "init.headers: $set\n"), init)
}
