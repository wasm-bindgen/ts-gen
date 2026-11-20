package fixtures.export_class_module.app

import fixtures.export_class_module.*

class Counter(ctx: DurableObjectState, env: Env) {
    suspend fun add(by: Double): Double = TODO("add")
}

class Greeter(ctx: DurableObjectState, env: Env) {
    fun greet(name: String): String = TODO("greet")
}

