package tsgen.kotlin

import java.io.ByteArrayOutputStream

/** An S-expression of the WAT subset ABI.md's grammar uses. */
sealed class Sx {
    /** A keyword, `$id`, number or `@abi.<name>`. */
    data class Atom(val text: String) : Sx() { override fun toString() = text }
    data class Str(val value: String) : Sx() { override fun toString() = "\"$value\"" }
    data class Lst(val items: List<Sx>) : Sx() {
        /** The leading atom, e.g. `import` or `@abi.ref`. */
        val head: String? get() = (items.firstOrNull() as? Atom)?.text
        val rest: List<Sx> get() = items.drop(1)
        fun lists(): List<Lst> = items.filterIsInstance<Lst>()
        fun find(head: String): Lst? = lists().firstOrNull { it.head == head }
        fun findAll(head: String): List<Lst> = lists().filter { it.head == head }
        fun str(i: Int): String = (items[i] as? Str)?.value ?: fail("expected a string at $i in $this")
        override fun toString() = items.joinToString(" ", "(", ")")
    }
}

fun fail(msg: String): Nothing = throw IllegalArgumentException(msg)

/** Reads one S-expression; line and block comments are skipped. */
class SxReader(private val src: String) {
    private var i = 0

    fun readAll(): List<Sx> {
        val out = mutableListOf<Sx>()
        while (true) {
            skip()
            if (i >= src.length) return out
            out += read()
        }
    }

    private fun skip() {
        while (i < src.length) {
            val c = src[i]
            when {
                c.isWhitespace() -> i++
                src.startsWith(";;", i) -> while (i < src.length && src[i] != '\n') i++
                src.startsWith("(;", i) -> {
                    var depth = 0
                    while (i < src.length) {
                        if (src.startsWith("(;", i)) { depth++; i += 2 }
                        else if (src.startsWith(";)", i)) { depth--; i += 2; if (depth == 0) break }
                        else i++
                    }
                }
                else -> return
            }
        }
    }

    private fun read(): Sx {
        skip()
        if (i >= src.length) fail("unexpected end of input")
        return when (src[i]) {
            '(' -> {
                i++
                val items = mutableListOf<Sx>()
                while (true) {
                    skip()
                    if (i >= src.length) fail("unclosed list")
                    if (src[i] == ')') { i++; break }
                    items += read()
                }
                Sx.Lst(items)
            }
            ')' -> fail("unexpected ) at offset $i")
            '"' -> Sx.Str(readString())
            else -> {
                val start = i
                while (i < src.length && !src[i].isWhitespace() && src[i] != '(' && src[i] != ')' && src[i] != '"' &&
                    !src.startsWith(";;", i)) i++
                Sx.Atom(src.substring(start, i))
            }
        }
    }

    /** A WAT string: its bytes are UTF-8, with `\hh` escaping single bytes. */
    private fun readString(): String {
        i++ // opening quote
        val bytes = ByteArrayOutputStream()
        fun put(s: String) = bytes.write(s.toByteArray(Charsets.UTF_8))
        while (true) {
            if (i >= src.length) fail("unterminated string")
            val c = src[i++]
            when (c) {
                '"' -> return bytes.toString(Charsets.UTF_8)
                '\\' -> {
                    val e = src[i++]
                    when (e) {
                        'n' -> bytes.write('\n'.code)
                        't' -> bytes.write('\t'.code)
                        'r' -> bytes.write('\r'.code)
                        '"' -> bytes.write('"'.code)
                        '\'' -> bytes.write('\''.code)
                        '\\' -> bytes.write('\\'.code)
                        'u' -> {
                            if (src[i] != '{') fail("bad \\u escape")
                            val end = src.indexOf('}', i)
                            val cp = src.substring(i + 1, end).replace("_", "").toInt(16)
                            i = end + 1
                            put(String(Character.toChars(cp)))
                        }
                        else -> {
                            val hex = "$e${src[i++]}"
                            bytes.write(hex.toInt(16))
                        }
                    }
                }
                else -> {
                    // Copy the char (and a surrogate pair's second half) as UTF-8.
                    if (c.isHighSurrogate() && i < src.length) put("$c${src[i++]}") else put(c.toString())
                }
            }
        }
    }
}
