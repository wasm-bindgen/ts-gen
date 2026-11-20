// Errors, in both directions.
package app

import workers.*

/** The arm of the built-in `err` a caught error is. */
fun describe(error: Err) = when (error) {
    is Err.TypeError -> "a TypeError (${error.value})"
    is Err.Error -> "an Error (${error.value})"
}

/** `/errors`: what JS throws and rejects with, caught as typed exceptions. */
suspend fun errors(env: Env): Response {
    // A plain `throws` is the built-in `err`: a TypeError, or any other error.
    val thrown = try {
        URL("not a url")
        "nothing"
    } catch (e: ErrException) {
        describe(e.error)
    }
    // A DOMException is not a TypeError, so it lands on `err`'s fallback arm.
    val fallback = try {
        structuredClone(HTMLRewriter())
        "nothing"
    } catch (e: ErrException) {
        describe(e.error)
    }
    // A rejected future throws from `await`.
    val rejected = try {
        env.BACKEND.fetch("not a url").await()
        "nothing"
    } catch (e: ErrException) {
        describe(e.error)
    }
    // An exception the Durable Object's Kotlin threw, back from JS.
    val crossed = try {
        counter(env).fetch("http://counter/counter/fail").await()
        "nothing"
    } catch (e: ErrException) {
        describe(e.error)
    }
    val reply = """
        URL("not a url") threw $thrown
        structuredClone(HTMLRewriter()) threw $fallback
        BACKEND.fetch("not a url") rejected with $rejected
        the counter's /fail rejected with $crossed
    """.trimIndent() + "\n"
    return text(reply)
}

/**
 * `/fail`: an exception out of the handler rejects its future. An
 * `ErrException` leaves as its arm's JS error, a TypeError here, and any
 * other Kotlin exception as the fallback arm, a JS Error.
 */
fun fail(path: String): Response = when (path) {
    "/fail/plain" -> throw IllegalStateException("a plain Kotlin exception")
    else -> throw ErrException(Err.TypeError("Kotlin says no".toJsString()))
}
