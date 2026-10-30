package tsgen.kotlin

val kotlinKeywords = setOf(
    "as", "break", "class", "continue", "do", "else", "false", "for", "fun", "if", "in",
    "interface", "is", "null", "object", "package", "return", "super", "this", "throw",
    "true", "try", "typealias", "typeof", "val", "var", "when", "while",
)

private val jsIdent = Regex("[A-Za-z_$][A-Za-z0-9_$]*")

/** Whether `s` can be a Kotlin name, backticked if need be, unchanged. */
fun isPlainIdent(s: String) = jsIdent.matches(s)

/**
 * A Kotlin identifier for a JS name: `[A-Za-z0-9_$]` (`$` needs [q]'s
 * backticks), no leading digit, not all underscores (reserved).
 */
fun sanitize(s: String): String {
    val t = s.map { if (it.isLetterOrDigit() && it.code < 128 || it == '_' || it == '$') it else '_' }.joinToString("")
    val u = if (t.isEmpty() || t[0].isDigit()) "_$t" else t
    return if (u.all { it == '_' }) "${u}1" else u
}

/** Backticks a name Kotlin would read as a keyword or that holds `$`. */
fun q(s: String) = if (s in kotlinKeywords || '$' in s) "`$s`" else s

fun camel(snake: String): String {
    val parts = snake.split('_').filter { it.isNotEmpty() }
    if (parts.isEmpty()) return snake
    return parts.first() + parts.drop(1).joinToString("") { it.replaceFirstChar(Char::uppercase) }
}

fun kdoc(indent: String, doc: String?, ts: String?): String {
    val lines = mutableListOf<String>()
    doc?.let { lines += it.lines() }
    if (ts != null) {
        if (lines.isNotEmpty()) lines += ""
        lines += "`" + ts.replace("`", "'") + "`"
    }
    if (lines.isEmpty()) return ""
    val body = lines.joinToString("\n") { l -> "$indent *" + (if (l.isBlank()) "" else " " + l.replace("*/", "*&#47;")) }
    return "$indent/**\n$body\n$indent */\n"
}
