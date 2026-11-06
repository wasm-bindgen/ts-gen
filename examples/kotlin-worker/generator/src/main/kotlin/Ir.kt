package tsgen.kotlin

/** The ts-gen IR (IR.md): classes, kinds, top-level members and provides. */
class Ir(val classes: List<Cls>, val kinds: List<KindDecl>, val functions: List<TopFn>, val provides: List<Provide>) {
    private val classIndex = classes.associateBy { it.module to it.name }
    private val kindIndex = kinds.associateBy { it.module to it.name }

    fun cls(module: String?, name: String) = classIndex[module to name]
    fun kind(module: String?, name: String) = kindIndex[module to name]

    /** The identity a type name denotes (IR.md, Name resolution): its module and binding name. */
    fun resolve(n: Named): Pair<String?, String> = when {
        n.qual == GLOBAL -> null to n.name
        n.qual != null -> n.qual to n.name
        n.site != null && (cls(n.site, n.name) != null || kind(n.site, n.name) != null) -> n.site to n.name
        else -> null to n.name
    }

    companion object { const val GLOBAL = "\u0000globalThis" }
}

enum class ClassKind { CLASS, SHAPE, STATIC }

class Cls(
    val name: String,
    val path: String,
    val module: String?,
    val kind: ClassKind,
    val tparams: List<String>,
    val extends: List<Named>,
    val doc: String?,
    val members: List<Member>,
) {
    val aliases = mutableListOf<String>()
}

class KindDecl(val name: String, val path: String, val module: String?, val fallback: Boolean, val arms: List<Arm>, val doc: String?)

class Arm(val name: String, val type: JsType, val aliases: List<String>, val doc: String?)

class TopFn(val module: String?, val member: Member)

enum class MemberKind(val receiver: Boolean = false, val static: Boolean = false) {
    CONSTRUCTOR, OBJECT,
    GET(receiver = true), SET(receiver = true), METHOD(receiver = true),
    STATIC_GET(static = true), STATIC_SET(static = true), STATIC_METHOD(static = true),
}

class Member(
    val name: String,
    val js: String,
    val kind: MemberKind,
    val tparams: List<String>,
    val params: List<Param>,
    val result: Type,
    val throws: Boolean,
    val errors: List<JsType>,
    val nothrow: Boolean,
    val cohort: String?,
    val optional: Boolean,
    val module: String?,
    val doc: String?,
) {
    val variadic get() = params.lastOrNull()?.rest == true
    val aliases = mutableListOf<String>()
}

class Param(val name: String, val type: Type, val rest: Boolean)

class Provide(val name: String, val js: List<String>, val params: List<Param>, val result: Type, val async: Boolean, val doc: String?)

sealed interface Type {
    object Void : Type
    object F64 : Type
    object I64 : Type
    object Bool : Type
    data class Js(val t: JsType) : Type
}

data class JsType(val head: Head, val nul: Boolean = false, val undef: Boolean = false) {
    val empty get() = nul || undef || head == Head.Empty
}

sealed interface Head {
    object Any : Head
    object Str : Head
    object Num : Head
    object Boolean : Head
    object BigInt : Head
    object Empty : Head
    data class Var(val name: String) : Head
    data class Callback(val params: List<Type>, val result: Type) : Head
    data class Tuple(val elems: List<JsType>) : Head
    data class StrLit(val value: String) : Head
    data class IntLit(val value: Long) : Head
}

/**
 * A class, kind or undeclared global by name. [qual] is the written
 * qualifier ([Ir.GLOBAL] for `globalThis.`, a specifier for `"spec".`),
 * [site] the module the reference appears in; [Ir.resolve] finds the identity.
 */
data class Named(val name: String, val qual: String?, val site: String?, val args: List<JsType>) : Head

class IrError(msg: String) : IllegalArgumentException(msg)

private enum class T { IDENT, STRING, NAT, PUNCT, DOC, EOF }

private class Tok(val t: T, val text: String, val line: Int)

private class Lexer(private val s: String) {
    private var i = 0
    private var line = 1
    val toks = mutableListOf<Tok>()

    init {
        while (true) {
            skip()
            if (i >= s.length) { toks += Tok(T.EOF, "", line); break }
            val c = s[i]
            when {
                s.startsWith("///", i) && !s.startsWith("////", i) -> {
                    var j = s.indexOf('\n', i).let { if (it < 0) s.length else it }
                    if (j > i && s[j - 1] == '\r') j--
                    var text = s.substring(i + 3, j)
                    if (text.startsWith(" ")) text = text.substring(1)
                    toks += Tok(T.DOC, text, line)
                    i = j
                }
                c.isLetter() || c == '_' || c == '$' -> {
                    val st = i
                    while (i < s.length && (s[i].isLetterOrDigit() || s[i] == '_' || s[i] == '$')) i++
                    toks += Tok(T.IDENT, s.substring(st, i), line)
                }
                c.isDigit() -> {
                    val st = i
                    while (i < s.length && s[i].isDigit()) i++
                    toks += Tok(T.NAT, s.substring(st, i), line)
                }
                c == '"' -> toks += Tok(T.STRING, string(), line)
                s.startsWith("=>", i) -> { toks += Tok(T.PUNCT, "=>", line); i += 2 }
                s.startsWith("...", i) -> { toks += Tok(T.PUNCT, "...", line); i += 3 }
                c in ";{}()<>,:=|.?[]-" -> { toks += Tok(T.PUNCT, c.toString(), line); i++ }
                else -> throw IrError("line $line: unexpected character '$c'")
            }
        }
    }

    private fun skip() {
        while (i < s.length) {
            val c = s[i]
            if (c == '\n') { line++; i++ }
            else if (c.isWhitespace()) i++
            else if (s.startsWith("//", i) && !(s.startsWith("///", i) && !s.startsWith("////", i))) {
                while (i < s.length && s[i] != '\n') i++
            } else break
        }
    }

    private fun string(): String {
        val sb = StringBuilder()
        i++
        while (true) {
            if (i >= s.length || s[i] == '\n') throw IrError("line $line: unterminated string")
            val c = s[i++]
            if (c == '"') break
            if (c != '\\') { sb.append(c); continue }
            when (val e = s.getOrNull(i++)) {
                '"' -> sb.append('"'); '\\' -> sb.append('\\')
                'n' -> sb.append('\n'); 'r' -> sb.append('\r'); 't' -> sb.append('\t')
                'u' -> {
                    if (s.getOrNull(i) != '{') throw IrError("line $line: bad \\u escape")
                    val end = s.indexOf('}', i)
                    if (end < 0) throw IrError("line $line: bad \\u escape")
                    sb.appendCodePoint(s.substring(i + 1, end).toInt(16))
                    i = end + 1
                }
                else -> throw IrError("line $line: bad escape \\$e")
            }
        }
        return sb.toString()
    }
}

/** Reads the IR text format (IR.md). */
class IrReader private constructor(text: String) {
    private val toks = Lexer(text).toks
    private var p = 0
    private val classes = mutableListOf<Cls>()
    private val kinds = mutableListOf<KindDecl>()
    private val functions = mutableListOf<TopFn>()
    private val provides = mutableListOf<Provide>()
    private val aliases = mutableListOf<Triple<String?, List<String>, List<String>>>()
    private var module: String? = null
    private var tvars: Set<String> = emptySet()

    companion object {
        fun read(text: String): Ir = IrReader(text).file()
        private val wasm = mapOf("void" to Type.Void, "f64" to Type.F64, "i64" to Type.I64, "bool" to Type.Bool)
        private val builtins = mapOf("Any" to Head.Any, "String" to Head.Str, "Number" to Head.Num, "Boolean" to Head.Boolean, "BigInt" to Head.BigInt)
    }

    private val tok get() = toks[p]
    private fun peek(n: Int = 1) = toks[minOf(p + n, toks.size - 1)]
    private fun fail(msg: String): Nothing = throw IrError("line ${tok.line}: $msg")
    private fun isP(s: String, t: Tok = tok) = t.t == T.PUNCT && t.text == s
    private fun isId(s: String, t: Tok = tok) = t.t == T.IDENT && t.text == s
    private fun isName(t: Tok = tok) = t.t == T.IDENT || t.t == T.STRING
    private fun next(): Tok = tok.also { if (it.t != T.EOF) p++ }
    private fun punct(s: String) { if (!isP(s)) fail("expected '$s', found '${tok.text}'"); p++ }
    private fun eatP(s: String) = isP(s).also { if (it) p++ }
    private fun eatId(s: String) = isId(s).also { if (it) p++ }
    private fun kw(s: String) { if (!eatId(s)) fail("expected '$s', found '${tok.text}'") }
    private fun ident(): String { if (tok.t != T.IDENT) fail("expected an identifier, found '${tok.text}'"); return next().text }
    private fun name(): String { if (!isName()) fail("expected a name, found '${tok.text}'"); return next().text }

    private fun path(): String {
        val sb = StringBuilder(ident())
        while (isP(".") && peek().t == T.IDENT) { p++; sb.append('.').append(ident()) }
        return sb.toString()
    }

    private fun segments(): List<String> {
        val l = mutableListOf(name())
        while (eatP(".")) l += name()
        return l
    }

    private fun docs(): String? {
        val l = mutableListOf<String>()
        while (tok.t == T.DOC) l += next().text
        return if (l.isEmpty()) null else l.joinToString("\n")
    }

    private fun tparams(): List<String> {
        if (!eatP("<")) return emptyList()
        val l = mutableListOf(ident())
        while (eatP(",")) l += ident()
        punct(">")
        return l
    }

    private fun file(): Ir {
        kw("ir")
        if (tok.t != T.NAT) fail("expected the format version")
        val v = next().text
        if (v != "1") fail("unsupported IR version $v")
        punct(";")
        while (tok.t != T.EOF) {
            val doc = docs()
            when {
                isId("module") && peek().t == T.STRING -> {
                    if (doc != null) fail("a doc before a module block")
                    p++
                    module = next().text
                    punct("{")
                    while (!isP("}")) { if (tok.t == T.EOF) fail("unterminated module block"); mdecl(docs()) }
                    p++
                    module = null
                }
                isId("provide") && peek().t != T.PUNCT -> provide(doc)
                else -> mdecl(doc)
            }
        }
        resolveAliases()
        return Ir(classes, kinds, functions, provides)
    }

    private fun mdecl(doc: String?) {
        when {
            isId("alias") && isName(peek()) -> {
                if (doc != null) fail("a doc before an alias")
                p++
                val l = segments()
                punct("=")
                val r = segments()
                punct(";")
                aliases += Triple(module, l, r)
            }
            (isId("shape") || isId("static")) && isId("class", peek()) -> classDecl(doc)
            isId("class") && peek().t == T.IDENT -> classDecl(doc)
            isId("kind") && peek().t == T.IDENT -> kindDecl(doc)
            isId("function") && isName(peek()) -> { p++; functions += TopFn(module, member(doc, MemberKind.STATIC_METHOD)) }
            isId("get") && isName(peek()) -> { p++; functions += TopFn(module, member(doc, MemberKind.STATIC_GET)) }
            isId("set") && isName(peek()) -> { p++; functions += TopFn(module, member(doc, MemberKind.STATIC_SET)) }
            tok.t == T.DOC -> fail("doc before nothing")
            else -> fail("expected a declaration, found '${tok.text}'")
        }
    }

    private fun classDecl(doc: String?) {
        val kind = when {
            eatId("shape") -> ClassKind.SHAPE
            eatId("static") -> ClassKind.STATIC
            else -> ClassKind.CLASS
        }
        kw("class")
        val path = path()
        val tps = tparams()
        val name = if (eatId("as")) path() else path
        tvars = tps.toSet()
        val ext = mutableListOf<Named>()
        if (eatId("extends")) {
            do { ext += (typeArm() as? Named) ?: fail("extends names a class") } while (eatP(","))
        }
        punct("{")
        val members = mutableListOf<Member>()
        val self = Named(name, module ?: Ir.GLOBAL, module, tps.map { JsType(Head.Var(it)) })
        while (!isP("}")) {
            if (tok.t == T.EOF) fail("unterminated class")
            members += classMember(docs(), path, self)
        }
        p++
        tvars = emptySet()
        classes += Cls(name, path, module, kind, tps, ext, doc, members)
    }

    private fun classMember(doc: String?, classPath: String, self: Named): Member {
        if (isId("new") && (isP("(", peek()) || isP("{", peek()) || isP("<", peek()) || peek().t == T.IDENT)) {
            p++
            if (eatP("{")) {
                punct("}")
                val res = if (eatP(":")) type() else Type.Js(JsType(self))
                return tail(doc, "new", "new", MemberKind.OBJECT, emptyList(), emptyList(), res, false)
            }
            val js = if (tok.t == T.IDENT) path() else classPath
            val tps = tparams()
            return withTvars(tps) {
                val ps = params()
                val res = if (eatP(":")) type() else Type.Js(JsType(self))
                tail(doc, js, "new", MemberKind.CONSTRUCTOR, tps, ps, res, false)
            }
        }
        val static = isId("static") && isName(peek())
        if (static) p++
        return member(doc, null, static)
    }

    private fun member(doc: String?, top: MemberKind?, static: Boolean = false): Member {
        val kind = top ?: when {
            isId("get") && isName(peek()) -> { p++; if (static) MemberKind.STATIC_GET else MemberKind.GET }
            isId("set") && isName(peek()) -> { p++; if (static) MemberKind.STATIC_SET else MemberKind.SET }
            else -> if (static) MemberKind.STATIC_METHOD else MemberKind.METHOD
        }
        val js = name()
        val optional = eatP("?")
        val tps = tparams()
        return withTvars(tps) {
            val getter = kind == MemberKind.GET || kind == MemberKind.STATIC_GET
            val ps = if (getter && !isP("(")) emptyList() else params()
            val res = when {
                getter -> { punct(":"); type() }
                eatP(":") -> type()
                else -> Type.Void
            }
            tail(doc, js, js, kind, tps, ps, res, optional)
        }
    }

    private fun <R> withTvars(tps: List<String>, f: () -> R): R {
        val saved = tvars
        tvars = saved + tps
        try { return f() } finally { tvars = saved }
    }

    private fun tail(doc: String?, js: String, defName: String, kind: MemberKind, tps: List<String>, ps: List<Param>, res: Type, optional: Boolean): Member {
        var throws = false
        val errors = mutableListOf<JsType>()
        fun errs() {
            do { errors += jsArm(typeArm()) } while (eatP("|"))
        }
        if (eatId("throws")) {
            throws = true
            if (!isP(";") && !(tok.t == T.IDENT && tok.text in setOf("nothrow", "cohort", "from", "as") && (isName(peek()) || isP(";", peek())))) errs()
        } else if (eatId("raises")) errs()
        val nothrow = eatId("nothrow")
        val cohort = if (eatId("cohort")) name() else null
        val from = if (isId("from") && peek().t == T.STRING) { p++; next().text } else null
        val name = if (eatId("as")) name() else defName
        punct(";")
        return Member(name, js, kind, tps, ps, res, throws, errors, nothrow, cohort, optional, from, doc)
    }

    private fun params(): List<Param> {
        punct("(")
        val l = mutableListOf<Param>()
        if (!isP(")")) do {
            val rest = eatP("...")
            val n = name()
            punct(":")
            l += Param(n, type(), rest)
        } while (eatP(","))
        punct(")")
        if (l.dropLast(1).any { it.rest }) fail("a rest parameter that is not last")
        return l
    }

    private fun kindDecl(doc: String?) {
        kw("kind")
        val path = path()
        val name = if (eatId("as")) path() else path
        val fallback = eatId("fallback")
        punct("{")
        val arms = mutableListOf<Arm>()
        while (!isP("}")) {
            if (tok.t == T.EOF) fail("unterminated kind")
            val d = docs()
            val n = name()
            punct("=")
            val t = (type() as? Type.Js)?.t ?: fail("a kind arm is a JS type")
            if (t.nul || t.undef) fail("a kind arm admits no Null or Undefined")
            val al = mutableListOf<String>()
            if (eatId("alias")) do { al += name() } while (eatP(","))
            punct(";")
            arms += Arm(n, t, al, d)
        }
        p++
        kinds += KindDecl(name, path, module, fallback, arms, doc)
    }

    private fun provide(doc: String?) {
        kw("provide")
        val js = segments()
        val ps = params()
        var async = false
        var res: Type = Type.Void
        if (eatP(":")) {
            if (isId("async") && !isP(";", peek()) && !isId("as", peek())) { p++; async = true }
            res = type()
        }
        val name = if (eatId("as")) name() else js.last()
        punct(";")
        provides += Provide(name, js, ps, res, async, doc)
    }

    private fun jsArm(t: Any): JsType = when (t) {
        is JsType -> t
        is Head -> JsType(t)
        else -> fail("a JS type is expected here")
    }

    /** A type: a wasm [Type], or a [JsType] wrapped in [Type.Js]. */
    private fun type(): Type {
        val first = typeArm()
        if (!isP("|")) return when (first) {
            is Type -> first
            is JsType -> Type.Js(first)
            is Head -> Type.Js(JsType(first))
            else -> error("unreachable")
        }
        val arms = mutableListOf(first)
        while (eatP("|")) arms += typeArm()
        var head: Head? = null
        var nul = false
        var undef = false
        for (a in arms) {
            val j = jsArm(if (a is Type) fail("a wasm type in a union") else a)
            nul = nul || j.nul
            undef = undef || j.undef
            if (j.head != Head.Empty) {
                if (head != null) fail("a union of two types (a kind)")
                head = j.head
            }
        }
        return Type.Js(JsType(head ?: Head.Empty, nul, undef))
    }

    /** One arm: a [Type] for a wasm type, else a [Head] or (for empties and parentheses) a [JsType]. */
    private fun typeArm(): Any {
        when {
            isP("(") -> {
                p++
                val l = mutableListOf<Type>()
                if (!isP(")")) do { l += type() } while (eatP(","))
                punct(")")
                if (eatP("=>")) return Head.Callback(l, type())
                val t = l.singleOrNull() ?: fail("expected '=>'")
                return (t as? Type.Js)?.t ?: fail("a parenthesised wasm type")
            }
            isP("[") -> {
                p++
                val l = mutableListOf<JsType>()
                if (!isP("]")) do { l += (type() as? Type.Js)?.t ?: fail("a wasm type in a tuple") } while (eatP(","))
                punct("]")
                return Head.Tuple(l)
            }
            tok.t == T.STRING && !isP(".", peek()) -> return Head.StrLit(next().text)
            isP("-") -> { p++; if (tok.t != T.NAT) fail("expected a number"); return Head.IntLit(-next().text.toLong()) }
            tok.t == T.NAT -> return Head.IntLit(next().text.toLong())
        }
        var qual: String? = null
        if (tok.t == T.STRING) { qual = next().text; punct(".") }
        else if (isId("globalThis") && isP(".", peek())) { p += 2; qual = Ir.GLOBAL }
        val path = path()
        val args = mutableListOf<JsType>()
        if (eatP("<")) {
            do { args += (type() as? Type.Js)?.t ?: fail("a wasm type argument") } while (eatP(","))
            punct(">")
        }
        if (qual == null && args.isEmpty()) {
            wasm[path]?.let { return it }
            builtins[path]?.let { return it }
            when (path) {
                "Null" -> return JsType(Head.Empty, nul = true)
                "Undefined" -> return JsType(Head.Empty, undef = true)
            }
            if (path in tvars) return Head.Var(path)
        }
        return Named(path, qual, module, args)
    }

    private fun resolveAliases() {
        for ((mod, l, r) in aliases) {
            val rhs = r.joinToString(".")
            val c = classes.find { it.module == mod && it.name == rhs }
            if (c != null) { c.aliases += l.joinToString("."); continue }
            if (r.size > 1) {
                val owner = classes.find { it.module == mod && it.name == r.dropLast(1).joinToString(".") }
                val m = owner?.members?.find { it.name == r.last() }
                if (m != null) { m.aliases += l.last(); continue }
            }
            val f = functions.find { it.module == mod && it.member.name == rhs }
                ?: throw IrError("alias ${l.joinToString(".")} names nothing: $rhs")
            f.member.aliases += l.joinToString(".")
        }
    }
}
