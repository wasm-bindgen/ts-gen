// Streams: a source, a transform, readers, and a streamed response body.
package app

import workers.*

/** A stream of `chunks`, from an underlying source whose `start` JS calls once. */
private fun streamOf(chunks: List<JsAny>): ReadableStream<JsAny?> {
    val source = UnderlyingSource<JsAny?>().apply {
        // `start` may return a promise, so it's a suspend lambda.
        setStart { controller ->
            for (chunk in chunks) controller.enqueue(chunk)
            controller.close()
        }
    }
    return ReadableStream(source)
}

/** Every chunk of `stream`, read to the end: `read()` resolves to `{ done, value }`. */
private suspend fun readAll(stream: ReadableStream<JsAny?>): List<JsAny?> {
    val reader = stream.getReader()
    val chunks = mutableListOf<JsAny?>()
    while (true) {
        val step = reader.read().await()
        if (step.done) break
        chunks += step.value
    }
    return chunks
}

/** `/stream`: words piped through an upper-casing transform, in a streamed reply. */
suspend fun stream(): Response {
    val words = streamOf(listOf("one", "two", "three").map { it.toJsString() })
    val upper = TransformStream<JsAny?, JsAny?>(Transformer<JsAny?, JsAny?>().apply {
        setTransform { chunk, controller -> controller.enqueue(chunk.toString().uppercase().toJsString()) }
    })
    // `pipeTo` is a future: it settles once the readable side below is read.
    val piped = words.pipeTo(upper.writable)
    // `tee()` returns a tuple of two streams.
    val (left, right) = upper.readable.tee()
    val shouted = readAll(left).joinToString(" ")
    val count = readAll(right).size
    piped.await()
    // The reply's body is itself a stream, of byte chunks.
    val encoder = TextEncoder()
    val lines = listOf("$count chunks through a TransformStream: $shouted\n", "this body was streamed in two chunks\n")
    return Response(BodyInit.ReadableStream(streamOf(lines.map { encoder.encode(it) })), ResponseInit().apply {
        setHeaders(Headers().apply { set("content-type", "text/plain; charset=utf-8") })
    })
}
