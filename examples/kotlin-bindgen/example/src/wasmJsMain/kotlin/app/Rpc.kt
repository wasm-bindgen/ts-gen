// An RPC entrypoint: `Greeter` extends the imported `WorkerEntrypoint`, so
// JS constructs it, and its methods are called through a stub.
package app

import std.Uint8Array
import std.buffer
import workers.*

class Greeter(private val ctx: ExecutionContext<JsAny?>, private val env: Env) {
    fun greet(name: String): String = greet(name, PlainOrLoud.Plain)

    /** `style` is `"plain" | "loud"`, a literal enum. */
    fun greet(name: String, style: PlainOrLoud): String = when (style) {
        PlainOrLoud.Plain -> "${env.GREETING}, $name"
        PlainOrLoud.Loud -> "${env.GREETING}, $name".uppercase() + "!"
    }

    /** The bytes arrive as a view of a buffer; the sum is a `u32`. */
    fun checksum(bytes: std.ArrayBufferView): UInt {
        val view = Uint8Array(bytes.buffer)
        return view.hex().chunked(2).sumOf { it.toUInt(16) }
    }

    /** A `bigint` result is a `JsBigInt`. */
    fun factorial(n: UInt): JsBigInt = (1..n.toLong()).fold(1L) { product, i -> product * i }.toJsBigInt()

    /** A tuple result, `[quotient, remainder]`; a thrown `ErrException` rejects the caller's future. */
    fun divide(a: UInt, b: UInt): Pair<Double, Double> {
        if (b == 0u) throw ErrException(Err.TypeError("division by zero".toJsString()))
        return Pair((a / b).toDouble(), (a % b).toDouble())
    }
}

/** `/rpc`: each `Greeter` method, called over RPC through the `GREETER` binding. */
suspend fun rpc(env: Env): Response {
    val greeter = env.GREETER
    // A `u8[]` parameter takes a ByteArray, copied into a fresh Uint8Array.
    val checksum = greeter.checksum(byteArrayOf(1, 2, 3, -6)).await()
    val (quotient, remainder) = greeter.divide(17u, 5u).await()
    val byZero = try {
        greeter.divide(1u, 0u).await()
        "resolved"
    } catch (e: ErrException) {
        "rejected (${e.message})"
    }
    val reply = """
        greet("Kotlin"): ${greeter.greet("Kotlin").await()}
        greet("Kotlin", "loud"): ${greeter.greet("Kotlin", PlainOrLoud.Loud).await()}
        checksum(bytes 1 2 3 250): $checksum
        factorial(20): ${greeter.factorial(20u).await()}
        divide(17, 5): $quotient remainder $remainder
        divide(1, 0): $byZero
    """.trimIndent() + "\n"
    return text(reply)
}
