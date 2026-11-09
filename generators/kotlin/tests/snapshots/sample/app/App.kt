package fixtures.sample.app

import fixtures.sample.*

suspend fun fetch(request: Response, body: Body): Response = TODO("fetch")

suspend fun stats_total(): Long = TODO("stats_total")

suspend fun stats_size(): ULong = TODO("stats_size")

class Counter(start: UInt) {
    fun increment(by: UInt): UInt = TODO("increment")
    val count: UInt get() = TODO("count")
    companion object {
        fun create(): fixtures.sample.app.Counter = TODO("create")
    }
}

