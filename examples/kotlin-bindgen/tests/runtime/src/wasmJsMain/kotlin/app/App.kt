// The probe's exports, called from JS through the bridge by test.mjs.
package app

import probe.Listener
import probe.Mode
import probe.ProbeError
import probe.ProbeErrorException
import probe.first
import probe.toCounter
import probe.toFnUIntToUInt
import probe.toJs
import probe.toMode
import tsgen.runtime.awaitCallback
import tsgen.runtime.delay

/** A tuple result: slot 1 is pushed through tw:ret. */
fun probe_split(n: UInt): Pair<UInt, Boolean> = Pair(n / 2u, n % 2u == 1u)

/** Throws `Busy` for 0, and a foreign exception for 1: both reach JS through set:ProbeError. */
fun probe_check(n: UInt): UInt = when (n) {
    0u -> throw ProbeErrorException(ProbeError.Busy("busy".toJsString()))
    1u -> throw IllegalStateException("one")
    else -> n * 10u
}

/** Whether a `Str?` box is none: either empty. */
fun probe_absent(v: JsString?): Boolean = v == null

/** Whether an import's `Str?` box result of `undefined` reads as none. */
fun probe_imported(): Boolean = probe.ext.none() == null

/** Resolves, or rejects with `Busy` through reject<ProbeError>, after a timer. */
suspend fun probe_settle(ok: Boolean): String {
    delay(1.0)
    if (!ok) throw ProbeErrorException(ProbeError.Busy("later".toJsString()))
    return "settled"
}

class Counter() {
    val n: UInt get() = 7u
}

/** Box helpers: an exported class, an enum and a callback cross erased positions as boxes. */
fun probe_counter(): JsAny? = Counter().toJs()

fun probe_count(v: JsAny?): UInt = v!!.toCounter().n

fun probe_mode(n: UInt): JsAny? = (if (n == 0u) Mode.Off else Mode.Count(n)).toJs()

fun probe_modeOf(v: JsAny?): UInt = when (val m = v!!.toMode()) {
    Mode.Off -> 0u
    is Mode.Count -> m.value
}

fun probe_adder(): JsAny? = { x: UInt -> x + 1u }.toJs()

fun probe_callBoxed(f: JsAny?, x: UInt): UInt = f!!.toFnUIntToUInt()(x)

/** A `Str?` box's none, `ref.null`, which JS sees as `undefined`. */
fun probe_none(): JsString? = null

/** Writes none to a default and a `@js(null)` `Str?` box. */
fun probe_writeNones() {
    probe.ext.seen(null)
    probe.ext.seenNull(null)
}

/** A callback returning a future is a suspend lambda, run as a task. */
suspend fun probe_asyncCallback(x: UInt): UInt = probe.ext.later({ n -> delay(1.0); n * 2u }, x).await()

/** A future parameter takes a suspend block, settled through resolve<str>. */
suspend fun probe_made(): String = probe.ext.settle { delay(1.0); "made" }.await()

/** A `u8[]` is a `ByteArray`, copied each way. */
fun probe_bytes(): UInt = probe.ext.bytes(byteArrayOf(1, 2, -6)).sumOf { it.toUByte().toUInt() }

/** An `arr<str>` is a `List<String>`. */
fun probe_words(): String = probe.ext.words(listOf("a", "b")).joinToString("")

/** A spread parameter is a vararg, and a type parameter no argument infers is dropped. */
fun probe_join(): String = probe.ext.join("-", "a", "b") + probe.ext.pick(3u)

/** Waits for a callback JS calls later, and is back on the loop after it. */
suspend fun probe_soon(): String {
    val word = awaitCallback<String> { resume -> probe.ext.soon { resume("soon") } }
    delay(1.0)
    return word
}

/** An enum's function arm is a callback Kotlin provides, of a type no parameter has. */
fun probe_listen(): String = probe.ext.listen(Listener.Call { "$it!" }, "a") + probe.ext.listen(Listener.Name("n"), "b")

/**
 * Host type arguments stay typed: a `Shelf<Str>` holds JS strings, a
 * member's `T` names its result's, and a `Shelf<Any?>` parameter takes any.
 */
suspend fun probe_generic(): String {
    val shelf = probe.ext.shelf()
    val word: JsString = shelf.first()
    val n: JsNumber = probe.ext.stock(41.toJsNumber()).await().first()
    return "$word ${n.toInt() + 1} ${probe.ext.size(shelf)}"
}

/** A future of an erased `T`, resolved by Kotlin and taken back. */
suspend fun probe_attempt(): String = probe.ext.attempt { "attempted".toJsString() }.await().toString()
