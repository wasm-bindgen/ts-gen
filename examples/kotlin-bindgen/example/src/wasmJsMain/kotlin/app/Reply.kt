// What the routes share.
package app

import std.IteratorYieldResult
import std.Uint8Array
import std.at
import std.done
import std.length
import std.next
import std.value
import workers.*

/** A request, with its two type parameters erased. */
typealias HttpRequest = Request<JsAny?, JsAny?>

/**
 * A plain-text response. The init is a dictionary, a JS object built field
 * by field, and its status a `u16` (annotate.d.ts).
 */
fun text(body: String, status: UShort = 200u): Response {
    val init = ResponseInit().apply {
        setStatus(status)
        setHeaders(Headers().apply { set("content-type", "text/plain; charset=utf-8") })
    }
    return Response(body, init)
}

/** The bytes of a JS `Uint8Array`, in hex. */
fun Uint8Array.hex(): String =
    (0 until length.toInt()).joinToString("") { at(it)!!.toString(16).padStart(2, '0') }

/**
 * The values of a JS iterator. Its element type is erased, and `next()`
 * returns `Any`, so each step is read as the yield result of `T`.
 */
fun <T : JsAny?> std.Iterator<JsAny?>.values(): Sequence<T> =
    generateSequence { next()!!.unsafeCast<IteratorYieldResult<T>>().takeUnless { it.done == true } }.map { it.value }
