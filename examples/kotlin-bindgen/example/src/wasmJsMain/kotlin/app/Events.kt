// The other handlers of the default export: `scheduled` and `queue`. The
// worker runs them on itself through the `SELF` service binding.
package app

import std.Date
import workers.*

private var lastCron = "(never)"
private var lastBatch = "(none)"

suspend fun scheduled(controller: ScheduledController, env: Env, ctx: ExecutionContext<JsAny?>) {
    lastCron = controller.cron
}

suspend fun queue(batch: MessageBatch<JsAny?>, env: Env, ctx: ExecutionContext<JsAny?>) {
    // `messages` is an `arr<Message>`, a List.
    for (message in batch.messages) message.ack()
    lastBatch = batch.messages.joinToString { "${it.id} (${it.body})" } + " from the queue \"${batch.queue}\""
}

/** `/scheduled`: a scheduled event, and what the handler saw of it. */
suspend fun runScheduled(env: Env): Response {
    val result = env.SELF.scheduled(ScheduledOptions().apply { setCron("*/5 * * * *") }).await()
    return text("scheduled ran with cron \"$lastCron\": outcome ${result.outcome}\n")
}

/** `/queue`: a batch of two messages, each a dictionary, acknowledged one by one. */
suspend fun runQueue(env: Env): Response {
    val messages = listOf("first", "second").mapIndexed { i, body ->
        QueueMessage().apply {
            id = "message-$i"
            timestamp = Date()
            this.body = body.toJsString()
            attempts = 1.0
        }
    }
    val result = env.SELF.queue("tour", messages).await()
    return text("queue ran: outcome ${result.outcome}, acknowledged $lastBatch\n")
}
