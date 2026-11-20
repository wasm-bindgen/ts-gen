// A Durable Object: the `Counter` class, exported as a JS class whose
// instances hold a Kotlin `Counter`, and the routes that reach it.
package app

import std.DateStatic
import std.forEach
import workers.*

/** The one instance the tour uses: an id from a name, then its stub. */
fun counter(env: Env): DurableObjectStub<JsAny?> {
    val id = env.COUNTER.idFromName("tour")
    return env.COUNTER.get(id)
}

/** `/counter/alarm`: sets an alarm, and waits for the object to say it has fired. */
suspend fun alarm(env: Env): Response {
    val stub = counter(env)
    suspend fun ask(path: String) = stub.fetch("http://counter/counter/alarm/$path").await().text().await()
    val before = ask("fired")
    val set = ask("set")
    var fired = before
    for (attempt in 1..100) {
        scheduler.wait(20.0).await()
        fired = ask("fired")
        if (fired != before) break
    }
    return text(set + fired)
}

class Counter(private val ctx: DurableObjectState<JsAny?>, private val env: Env) {
    private val storage = ctx.storage

    suspend fun fetch(request: HttpRequest): Response {
        val path = URL(request.url).pathname.removePrefix("/counter")
        return when (path) {
            "/increment" -> increment()
            "/list" -> list()
            "/reset" -> reset()
            "/sql" -> sql(request.headers.get("user-agent") ?: "(none)")
            "/alarm/set" -> setAlarm()
            "/alarm/fired" -> text("alarms fired: ${stored("alarms")}\n")
            "/ws" -> hibernate()
            "/fail" -> throw ErrException(Err.TypeError("the counter doesn't do $path".toJsString()))
            else -> text("no counter route for $path\n", 404u)
        }
    }

    /** A stored number: `get<T>` resolves to `T?`, none for a key never put. */
    private suspend fun stored(key: String): Int = storage.get<JsNumber>(key).await()?.toInt() ?: 0

    /** KV storage, read and written in one transaction: its closure is a suspend lambda. */
    private suspend fun increment(): Response {
        val count = storage.transaction { txn ->
            val next = (txn.get<JsNumber>("count").await()?.toInt() ?: 0) + 1
            txn.put("count", next.toJsNumber()).await()
            txn.put("at:$next", DateStatic.now().toJsNumber()).await()
            next.toJsNumber()
        }.await()
        return text("count: $count (in ${env.GREETING.substringAfter("from ")})\n")
    }

    /** `list` with options resolves to a JS `Map`, whose `forEach` takes a callback. */
    private suspend fun stamps(): List<String> {
        val entries = storage.list<JsNumber>(DurableObjectListOptions().apply { setPrefix("at:") }).await()
        val keys = mutableListOf<String>()
        entries.forEach { _, key, _ -> keys += key.toString() }
        return keys
    }

    private suspend fun list(): Response {
        val keys = stamps()
        return text("${keys.size} increments recorded: ${keys.joinToString()}\n")
    }

    /** `delete` of one key resolves to whether it was there; of several, to how many. */
    private suspend fun reset(): Response {
        val had = storage.delete("count").await()
        val stamps = storage.delete(stamps()).await()
        return text("reset: count ${if (had) "deleted" else "wasn't set"}, ${stamps.toInt()} increments forgotten\n")
    }

    /** SQL storage: `exec` takes its bindings as varargs, and returns a cursor. */
    private fun sql(agent: String): Response {
        val sql = storage.sql
        sql.exec<JsAny?>("CREATE TABLE IF NOT EXISTS visits (id INTEGER PRIMARY KEY, agent TEXT)")
        sql.exec<JsAny?>("INSERT INTO visits (agent) VALUES (?)", agent.toJsString())
        // `exec<T>` names its rows' type: here `Visit` (env.d.ts).
        val cursor = sql.exec<Visit>("SELECT id, agent FROM visits ORDER BY id DESC LIMIT ?", 3.toJsNumber())
        val out = StringBuilder("columns ${cursor.columnNames}\n")
        // The cursor is a JS iterator of its rows.
        while (true) {
            val step = cursor.next()
            if (step.done == true) break
            val visit = step.value!!
            out.append("  visit ${visit.id} by ${visit.agent}\n")
        }
        out.append("${cursor.rowsRead.toInt()} rows read\n")
        return text(out.toString())
    }

    private suspend fun setAlarm(): Response {
        storage.setAlarm(DateStatic.now() + 20.0).await()
        return text("alarm set for 20ms from now\n")
    }

    /** The alarm handler, an export of its own: the runtime calls it when the alarm fires. */
    suspend fun alarm(alarmInfo: AlarmInvocationInfo) {
        if (!alarmInfo.isRetry) storage.put("alarms", (stored("alarms") + 1).toJsNumber()).await()
    }

    /** A hibernating WebSocket: the runtime holds it, and wakes `webSocketMessage`. */
    private fun hibernate(): Response {
        val pair = WebSocketPair()
        ctx.acceptWebSocket(pair._1, listOf("tour"))
        return Response(null, ResponseInit().apply {
            setStatus(101u)
            webSocket = pair._0
        })
    }

    suspend fun webSocketMessage(ws: WebSocket, message: String) {
        val sockets = ctx.getWebSockets("tour").size
        ws.send("the counter heard \"$message\" ($sockets hibernating)")
        ws.close(1000.0, "done")
    }

    suspend fun webSocketMessage(ws: WebSocket, message: std.ArrayBuffer) {
        ws.send(message)
    }
}

/** `/ws`: a WebSocket the worker itself accepts, with a listener for its messages. */
fun webSocket(): Response {
    val pair = WebSocketPair()
    val server = pair._1
    server.accept()
    // A callback JS calls for every message.
    server.addEventListener("message") { event ->
        val data = event.unsafeCast<MessageEvent>().data
        server.send("the worker heard \"$data\"")
        server.close(1000.0, "done")
    }
    return Response(null, ResponseInit().apply {
        setStatus(101u)
        webSocket = pair._0
    })
}
