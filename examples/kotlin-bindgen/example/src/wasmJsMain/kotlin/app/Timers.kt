// Timers and tasks: scheduler.wait, setTimeout, queueMicrotask, waitUntil.
package app

import tsgen.runtime.awaitCallback
import workers.*

/** How many `waitUntil` tasks have finished. */
private var finished = 0

/** `/timers`: waits three ways, and leaves a task running past the reply. */
suspend fun timers(ctx: ExecutionContext<JsAny?>): Response {
    // A future of unit.
    scheduler.wait(5.0).await()
    // A callback JS calls once, after 5ms: the task waits for it.
    awaitCallback<Unit> { resume -> setTimeout({ resume(Unit) }, 5.0) }
    // A microtask: its callback returns `Any`.
    val order = mutableListOf("before")
    awaitCallback<Unit> { resume ->
        queueMicrotask {
            order += "microtask"
            resume(Unit)
            null
        }
        order += "after"
    }
    // `waitUntil` takes a future: a suspend block, run as its own task.
    val before = finished
    ctx.waitUntil {
        scheduler.wait(5.0).await()
        finished++
        null
    }
    val reply = """
        scheduler.wait(5) resolved, then setTimeout(5) called back
        queueMicrotask ran in the order: ${order.joinToString()}
        waitUntil: $before tasks had finished, and one more is running
    """.trimIndent() + "\n"
    return text(reply)
}
