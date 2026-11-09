package fixtures.export_class.app

import fixtures.export_class.*

fun version(): String = TODO("version")

fun rename(from: String?) { TODO("rename") }

fun rename(from: String?, to: String?) { TODO("rename") }

class Counter(ctx: DurableObjectState, env: Env) {
    suspend fun increment(by: Double): Double = TODO("increment")
    val count: Double get() = TODO("count")
    var label: String? = TODO("label")
    suspend fun fetch(request: Request): Response = TODO("fetch")
    companion object {
        fun restore(id: String): fixtures.export_class.app.Counter = TODO("restore")
    }
}

class Registry() {
    val size: Double get() = TODO("size")
    companion object {
        fun create(): fixtures.export_class.app.Registry = TODO("create")
    }
}

