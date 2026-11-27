// Subrequests: a service binding, the global fetch, a timeout, and cancellation.
package app

import tsgen.runtime.delay
import workers.*

/** `/fetch`: requests to the second service of config.capnp (backend.js). */
suspend fun subrequests(env: Env): Response {
    // A service binding is a Fetcher.
    val hello = env.BACKEND.fetch("http://backend/hello").await()

    // The global fetch, with an init: its cache mode is a literal enum.
    val init = RequestInit<RequestInitCfProperties>().apply {
        setMethod("POST")
        setHeaders(Headers().apply { set("content-type", "text/plain") })
        body = BodyInit.Str("ping")
        setCache(NoStoreOrNoCache.NoStore)
    }
    val posted = fetch("http://backend/post", init).await()

    // A caller's own signal is kept: the bridge joins it with the one it
    // aborts on a cancel, so the timeout still rejects the future.
    val timed = RequestInit<RequestInitCfProperties>().apply { signal = AbortSignalStatic.timeout(50.0) }
    val timeout = try {
        fetch("http://backend/slow", timed).await()
        "nothing"
    } catch (e: ErrException) {
        describe(e.error)
    }
    val timedOut = aborts(env)

    // Cancelling a pending future is a request: the bridge aborts a signal
    // of its own, which the backend sees as its request aborting, and the
    // future rejects as the fetch does. With no init to carry the signal,
    // the bridge makes one.
    val pending = fetch("http://backend/slow")
    delay(50.0)
    pending.cancel()
    val cancelled = aborts(env)

    val reply = """
        BACKEND.fetch: ${hello.status} ${hello.text().await()}
        fetch with an init: ${posted.status} ${posted.text().await()}
        a fetch with AbortSignal.timeout(50) rejected with $timeout
        the backend saw $timedOut of its requests abort
        a cancelled fetch: the backend saw $cancelled of its requests abort
    """.trimIndent() + "\n"
    return text(reply)
}

/** How many of its requests the backend saw abort, once it has seen one. */
private suspend fun aborts(env: Env): String {
    for (attempt in 1..50) {
        val seen = env.BACKEND.fetch("http://backend/aborts").await().text().await()
        if (seen != "0") return seen
        delay(20.0)
    }
    return "0"
}
