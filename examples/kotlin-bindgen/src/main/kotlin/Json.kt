package tsgen.kotlin

class JsonError(message: String) : Exception(message)

/**
 * A strict JSON reader: objects become ordered maps, numbers [Long], and
 * the rest [String], [Boolean], lists and `null`. The lowered IR has no
 * fractional numbers.
 */
class JsonReader private constructor(private val s: String) {
    private var i = 0

    companion object {
        fun read(s: String): Any? {
            val r = JsonReader(s)
            val v = r.value()
            r.ws()
            if (r.i != s.length) r.fail("trailing input")
            return v
        }
    }

    private fun fail(msg: String): Nothing = throw JsonError("$msg at offset $i")

    private fun ws() {
        while (i < s.length && s[i] in " \n\r\t") i++
    }

    private fun expect(c: Char) {
        ws()
        if (i >= s.length || s[i] != c) fail("expected '$c'")
        i++
    }

    private fun value(): Any? {
        ws()
        if (i >= s.length) fail("unexpected end")
        return when (s[i]) {
            '{' -> obj()
            '[' -> arr()
            '"' -> str()
            't' -> word("true", true)
            'f' -> word("false", false)
            'n' -> word("null", null)
            else -> num()
        }
    }

    private fun word(w: String, v: Any?): Any? {
        if (!s.startsWith(w, i)) fail("bad literal")
        i += w.length
        return v
    }

    private fun num(): Long {
        val start = i
        if (s[i] == '-') i++
        while (i < s.length && s[i].isDigit()) i++
        return s.substring(start, i).toLongOrNull() ?: fail("bad number")
    }

    private fun str(): String {
        expect('"')
        val b = StringBuilder()
        while (true) {
            if (i >= s.length) fail("unterminated string")
            val c = s[i++]
            when (c) {
                '"' -> return b.toString()
                '\\' -> {
                    val e = s[i++]
                    when (e) {
                        '"', '\\', '/' -> b.append(e)
                        'n' -> b.append('\n')
                        'r' -> b.append('\r')
                        't' -> b.append('\t')
                        'b' -> b.append('\b')
                        'f' -> b.append('\u000c')
                        'u' -> {
                            b.append(s.substring(i, i + 4).toInt(16).toChar())
                            i += 4
                        }
                        else -> fail("bad escape")
                    }
                }
                else -> b.append(c)
            }
        }
    }

    private fun arr(): List<Any?> {
        expect('[')
        val out = mutableListOf<Any?>()
        ws()
        if (s[i] == ']') { i++; return out }
        while (true) {
            out += value()
            ws()
            when (s[i++]) {
                ',' -> continue
                ']' -> return out
                else -> fail("expected ',' or ']'")
            }
        }
    }

    private fun obj(): Map<String, Any?> {
        expect('{')
        val out = LinkedHashMap<String, Any?>()
        ws()
        if (s[i] == '}') { i++; return out }
        while (true) {
            ws()
            val k = str()
            expect(':')
            out[k] = value()
            ws()
            when (s[i++]) {
                ',' -> continue
                '}' -> return out
                else -> fail("expected ',' or '}'")
            }
        }
    }
}
