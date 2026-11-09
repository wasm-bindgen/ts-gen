// The worker: the default export's `fetch`, generated as a suspend
// function the runtime runs as a task on its loop. It awaits the request
// body (a JS future, through tw:future poll and take<str>), then a timer
// (through tw:loop/next_sleep) before replying. `FormData.get` returns a
// payload enum, two values spilled through tw:ret, and a bad URL throws a
// TypeError, read through tw:err as `err`'s typed exception.
package app

import tsgen.runtime.delay
import workers.*

suspend fun fetch(request: Request<JsAny?, JsAny?>, env: JsAny?, ctx: ExecutionContext<JsAny?>): Response {
    val body = request.text().await()
    val header = request.headers.get("x-kotlin")
    delay(1.0)
    val form = FormData()
    form.append("k", "v")
    fun entry(v: FileOrStr?) = when (v) {
        is FileOrStr.Str -> "str ${v.value}"
        is FileOrStr.File -> "file"
        null -> "none"
    }
    val url = try {
        URL("not a url")
        "parsed"
    } catch (e: ErrException) {
        when (e.error) {
            is Err.TypeError -> "TypeError"
            is Err.Error -> "Error"
        }
    }
    val reply = "${request.method} ${request.url}\n" +
        "x-kotlin: ${header ?: "(none)"}\n" +
        "GREETING: ${env!!.unsafeCast<Env>().GREETING}\n" +
        "body: $body\n" +
        "form: ${entry(form.get("k"))}, ${entry(form.get("missing"))}\n" +
        "url: $url\n"
    val headers = Headers()
    headers.set("content-type", "text/plain; charset=utf-8")
    val init = ResponseInit()
    init.setStatus(200.0)
    init.setHeaders(headers)
    return Response(reply, init)
}
