// Subrequests: a service binding, the global fetch, and cancellation.
package app

import tsgen.runtime.delay
import workers.*

/** `/fetch`: requests to the second service of config.capnp (backend.js). */
suspend fun subrequests(env: Env): Response {
    // A service binding is a Fetcher.
    val hello = env.BACKEND.fetch("http://backend/hello").await()

    // The global fetch, with an init: its cache mode is a literal enum.
    val init = RequestInit<JsAny?>().apply {
        setMethod("POST")
        setHeaders(Headers().apply { set("content-type", "text/plain") })
        body = BodyInit.Str("ping")
        setCache(NoStoreOrNoCache.NoStore)
    }
    val posted = fetch("http://backend/post", init).await()

    // Dropping a pending future cancels it: the bridge aborts the signal
    // it put in the init, which the backend sees as its request aborting.
    val pending = fetch("http://backend/slow", RequestInit())
    delay(50.0)
    pending.drop()
    var aborts = "0"
    for (attempt in 1..50) {
        aborts = env.BACKEND.fetch("http://backend/aborts").await().text().await()
        if (aborts != "0") break
        delay(20.0)
    }

    val reply = """
        BACKEND.fetch: ${hello.status} ${hello.text().await()}
        fetch with an init: ${posted.status} ${posted.text().await()}
        a dropped fetch: the backend saw $aborts of its requests abort
    """.trimIndent() + "\n"
    return text(reply)
}
