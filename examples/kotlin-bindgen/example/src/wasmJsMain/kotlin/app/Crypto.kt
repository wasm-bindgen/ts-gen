// Web Crypto and the encoding APIs.
package app

import std.Uint8Array
import std.buffer
import workers.*

/** `/crypto`: a digest, random values, and text and base64 encoding. */
suspend fun crypto(): Response {
    val bytes = TextEncoder().encode("hello kotlin")
    // The data is a `BufferSource`, an ArrayBuffer or a view of one: a
    // `buffer`, written as bytes.
    val utf8 = "hello kotlin".encodeToByteArray()
    val digest = crypto.subtle.digest("SHA-256", utf8).await()
    // `getRandomValues<T>` fills the view it's given, and returns it as `T`.
    val random = crypto.getRandomValues(Uint8Array(8.0))
    val base64 = btoa("hello kotlin")
    // `atob` is annotated `@throws {DOMException}`: its own exception type.
    val invalid = try {
        atob("not base64!")
    } catch (e: DOMExceptionErrException) {
        when (val error = e.error) {
            is DOMExceptionErr.DOMException -> "DOMException (${error.value})"
            is DOMExceptionErr.TypeError -> "TypeError (${error.value})"
            is DOMExceptionErr.Error -> "Error (${error.value})"
        }
    }
    val reply = """
        "hello kotlin" is ${bytes.hex()} in UTF-8
        decoded again: ${TextDecoder().decode(utf8)}
        SHA-256: ${Uint8Array(digest).hex()}
        8 random bytes: ${random.hex().length / 2} read back
        a UUID: ${crypto.randomUUID().length} characters
        base64: $base64, and back: ${atob(base64)}
        atob("not base64!"): $invalid
    """.trimIndent() + "\n"
    return text(reply)
}
