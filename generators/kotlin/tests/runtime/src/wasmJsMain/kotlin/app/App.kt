// The probe's exports, called from JS through the bridge by test.mjs.
package app

import probe.Mode
import probe.ProbeError
import probe.ProbeErrorException
import probe.toCounter
import probe.toFnUIntToUInt
import probe.toJs
import probe.toMode
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
