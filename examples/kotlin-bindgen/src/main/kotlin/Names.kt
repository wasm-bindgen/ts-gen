package tsgen.kotlin

val kotlinKeywords = setOf(
    "as", "break", "class", "continue", "do", "else", "false", "for", "fun", "if", "in",
    "interface", "is", "null", "object", "package", "return", "super", "this", "throw",
    "true", "try", "typealias", "typeof", "val", "var", "when", "while",
)

/**
 * A Kotlin identifier for any name: `[A-Za-z0-9_]`, every other character
 * replaced by `_` with runs collapsed, and no leading digit. A name with no
 * letters or digits is spelled out instead.
 */
fun sanitize(s: String): String {
    val t = buildString {
        for (c in s) {
            val ok = c.isLetterOrDigit() && c.code < 128 || c == '_'
            if (ok) append(c) else if (isEmpty() || last() != '_') append('_')
        }
    }.trimEnd('_')
    if (t.none { it.isLetterOrDigit() } && s.isNotEmpty()) return spell(s)
    val u = if (t.isEmpty() || t[0].isDigit()) "_$t" else t
    return if (u.all { it == '_' }) "${u}1" else u
}

private val symbolNames = mapOf(
    '$' to "dollar", '&' to "amp", '+' to "plus", '`' to "backtick", '\'' to "quote", '_' to "underscore",
    '*' to "star", '-' to "minus", '@' to "at", '#' to "hash", '%' to "percent", '!' to "bang", '?' to "question",
)

/** A name with no letters or digits (`$&`, RegExp's legacy statics), spelled out: `dollarAmp`. */
private fun spell(s: String) = s.mapIndexed { i, c ->
    val w = symbolNames[c] ?: "u${c.code.toString(16)}"
    if (i == 0) w else upperFirst(w)
}.joinToString("")

/** Backticks a name Kotlin would read as a keyword. */
fun q(s: String) = if (s in kotlinKeywords) "`$s`" else s

fun upperFirst(s: String) = s.replaceFirstChar(Char::uppercase)

/** A Kotlin string literal: escapes `"`, `\` and `$`. */
fun lit(s: String) = "\"" + s.replace("\\", "\\\\").replace("\"", "\\\"").replace("\$", "\\\$") + "\""

fun kdoc(indent: String, doc: String?): String {
    val lines = doc?.lines() ?: return ""
    val body = lines.joinToString("\n") { l -> "$indent *" + (if (l.isBlank()) "" else " " + l.replace("*/", "*&#47;")) }
    return "$indent/**\n$body\n$indent */\n"
}
