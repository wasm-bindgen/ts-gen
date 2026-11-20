// The request and response types: Request, Headers, URL, FormData, JSON.
package app

import workers.*

/** `/echo`: the request read back, in a response with its own status and headers. */
suspend fun echo(request: HttpRequest): Response {
    val headers = request.headers
    // `get` returns `str?`: a header that isn't there is null.
    val agent = headers.get("user-agent") ?: "(none)"
    // `forEach` takes a callback, which JS calls once per header.
    val custom = mutableListOf<String>()
    headers.forEach { value, name, _ -> if (name.startsWith("x-")) custom += "$name=$value" }
    // `entries()` is a JS iterator of `[name, value]` tuples.
    val count = headers.entries().values().count()
    val body = request.text().await()
    val reply = """
        ${request.method} ${request.url}
        user-agent: $agent
        x- headers: ${custom.joinToString()}
        $count headers in all
        body: $body
    """.trimIndent() + "\n"
    val init = ResponseInit().apply {
        setStatus(200u)
        setStatusText("Echoed")
        setHeaders(Headers().apply {
            set("content-type", "text/plain; charset=utf-8")
            set("x-body-length", body.length.toString())
        })
    }
    return Response(reply, init)
}

/** `/url`: a URL's parts and its search parameters. */
fun url(url: URL): Response {
    val params = url.searchParams
    val out = StringBuilder("${url.pathname} on ${url.host} has ${params.size} parameters\n")
    for (entry in params.entries().values()) {
        out.append("  ${entry[0]} = ${entry[1]}\n")
    }
    // An `arr<str>` is a List<String>.
    out.append("every tag: ${params.getAll("tag")}\n")
    // The base is a `str | URL` union: either arm of `StrOrURL`.
    val next = URL("../next?page=2", StrOrURL.URL(url))
    next.searchParams.append("from", url.pathname)
    next.hash = "top"
    out.append("a relative URL: ${next.href}\n")
    // A constructor that throws: the JS TypeError is an `ErrException`.
    val invalid = try {
        URL("not a url").href
    } catch (e: ErrException) {
        describe(e.error)
    }
    out.append("URL(\"not a url\") is $invalid\n")
    return text(out.toString())
}

/** `/form`: a posted form's fields. An entry is `File | str`, a payload enum, or null. */
suspend fun form(request: HttpRequest): Response {
    val form = request.formData().await()
    val out = StringBuilder()
    for (name in listOf("title", "upload", "missing")) {
        val field = when (val entry = form.get(name)) {
            is FileOrStr.Str -> "the string \"${entry.value}\""
            is FileOrStr.File -> {
                val file = entry.value
                val bytes = file.bytes().await()
                "the file ${file.name} (${file.type}), ${file.size} bytes: ${bytes.hex()}"
            }
            null -> "not in the form"
        }
        out.append("$name: $field\n")
    }
    return text(out.toString())
}

/** `/json`: a JSON body read as an `Order` (env.d.ts), and a JSON reply. */
suspend fun json(request: HttpRequest): Response {
    // `json()` resolves to an erased `T`: naming it reads the value as that type.
    // A body that isn't JSON rejects the future, which throws from `await`.
    val order = try {
        request.json<Order>().await()
    } catch (e: ErrException) {
        return text("the body is not JSON: ${describe(e.error)}\n", 400u)
    }
    // `JSON.parse` returns `Any`, a value of no known type, cast to read it.
    val copy = JSON.parse(JSON.stringify(order))!!.unsafeCast<Order>()
    copy.quantity = order.quantity * 2u
    copy.tags = order.tags + "doubled"
    val invalid = try {
        JSON.parse("{oops")
        "parsed"
    } catch (e: SyntaxErrException) {
        "a SyntaxError"
    }
    copy.setNote("${order.quantity} x ${order.item}, tags ${order.tags}; parsing \"{oops\" threw $invalid")
    return ResponseStatic.json(copy)
}
