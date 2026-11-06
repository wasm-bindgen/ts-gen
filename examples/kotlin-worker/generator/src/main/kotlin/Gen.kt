package tsgen.kotlin

/**
 * Writes Kotlin/Wasm bindings for a ts-gen IR file.
 *
 * * Flat layer: one `@JsFun` external per binding, in the package that
 *   makes its fully qualified name the import name (`<class>.<member>`),
 *   so Kotlin imports it as `"js_code" "<name>"` and `ts-gen embed
 *   --bindings-module js_code` links it to the bindings.
 * * Typed layer, in [pkg]: an `external interface` per class and kind
 *   (a JS reference, so it can be a type argument: `Promise<Response>`),
 *   members as extensions forwarding to the flat layer, constructors as
 *   factory functions named like the class, statics in `object <Class>Static`.
 * * Entry, in `[pkg].provided`: a `@JsExport` per provide, calling the
 *   user's function of the same name in [appPkg].
 *
 * Names: with [verbatim], Kotlin names are the IR's binding names (IR
 * written with a Kotlin naming preset). Otherwise they are derived here:
 * the JS name (`set<Name>` for a setter), and on a Kotlin clash the
 * binding name camel-cased (`jsonWithInit`).
 */
class Gen(private val ir: Ir, private val pkg: String, private val appPkg: String, private val verbatim: Boolean = false) {
    private val bindings = "./bindings.wasm"
    private val classes = ir.classes.filter { it.module == null }.associateBy { it.name }
    private val kinds = ir.kinds.filter { it.module == null }.associateBy { it.name }
    private val skipped = mutableListOf<String>()
    private val bound = mutableListOf<Bound>()
    private val ktNames = mutableMapOf<Any, String>() // Cls or KindDecl -> Kotlin name
    private val holders = mutableMapOf<Any, String>() // Cls or KindDecl -> statics object
    private val classifiers = mutableSetOf<String>()

    /** A binding the flat layer declares. [owner] is null at top level. */
    private inner class Bound(val owner: Cls?, val m: Member) {
        val import = if (owner == null) m.name else "${owner.name}.${m.name}"
        lateinit var alias: String
    }

    /** A kind's discriminator (IR.md, Bridge): its arm index for a value, -1 for none. */
    private inner class Disc(val k: KindDecl) {
        val import = "kind:${k.name}"
        lateinit var alias: String
    }

    private val discs = kinds.values.filter { k -> k.name.split('.').all(::isPlainIdent) }.map { Disc(it) }
    private val armTypes = mutableMapOf<KindDecl, String>()

    init {
        for (c in ir.classes) for (m in c.members) consider(c, m)
        for (f in ir.functions) consider(null, f.member, f.module)
        val used = mutableSetOf<String>()
        fun alias(import: String): String {
            var a = "abi_" + sanitize(import.replace(".", "_")).replace("$", "_")
            while (!used.add(a)) a += "_"
            return a
        }
        for (b in bound) b.alias = alias(b.import)
        for (d in discs) d.alias = alias(d.import)
        for ((n, d) in classes.map { it.key to it.value } + kinds.map { it.key to it.value }) {
            var k = sanitize(n.replace('.', '_'))
            while (!classifiers.add(k)) k += "_"
            ktNames[d] = k
        }
        for (c in classes.values) if (c.kind != ClassKind.STATIC && bound.any { it.owner === c && it.m.kind.static }) holders[c] = holderName(c)
        for (d in discs) {
            var a = ktNames.getValue(d.k) + "Arm"
            while (!classifiers.add(a)) a += "_"
            armTypes[d.k] = a
        }
    }

    private fun holderName(n: Any): String {
        var h = ktNames.getValue(n) + "Static"
        while (!classifiers.add(h)) h += "_"
        return h
    }

    private fun consider(c: Cls?, m: Member, topModule: String? = null) {
        val module = m.module ?: c?.module ?: topModule
        val name = if (c == null) m.name else "${c.name}.${m.name}"
        val why = when {
            module != null -> "module-scoped (\"$module\")"
            m.variadic -> "variadic"
            !name.split('.').all(::isPlainIdent) -> "name is not a Kotlin identifier"
            c?.kind == ClassKind.STATIC && m.kind.receiver -> "instance member of a namespace"
            else -> null
        }
        if (why != null) skipped += "\"${module ?: bindings}\" \"$name\": $why" else bound += Bound(c, m)
    }

    private fun T(d: Any) = "$pkg.${ktNames.getValue(d)}"

    private val anyN = "kotlin.js.JsAny?"

    /** A declared class or kind of this IR's global scope that [n] names, or null. */
    private fun declared(n: Named): Any? {
        val (mod, name) = ir.resolve(n)
        if (mod != null) return null
        return classes[name]?.takeIf { it.kind != ClassKind.STATIC } ?: kinds[name]
    }

    /** Whether the flat layer must take or return [j] as `JsAny?`. */
    private fun flatNullable(j: JsType): Boolean = j.empty || j.head is Head.Var

    /** The Kotlin JS-reference type for [j], usable as a type argument. [vars] are the type parameters in scope. */
    private fun jsRef(j: JsType, vars: Set<String>): String {
        val h = j.head
        val base = when (h) {
            Head.Any, Head.Empty, is Head.Callback -> "kotlin.js.JsAny"
            Head.Str, is Head.StrLit -> "kotlin.js.JsString"
            Head.Num, is Head.IntLit -> "kotlin.js.JsNumber"
            Head.Boolean -> "kotlin.js.JsBoolean"
            Head.BigInt -> "kotlin.js.JsBigInt"
            is Head.Var -> if (h.name in vars) h.name else anyN
            is Head.Tuple -> {
                val es = h.elems.map { jsRef(it, vars) }.distinct()
                "kotlin.js.JsArray<${es.singleOrNull() ?: anyN}>"
            }
            is Named -> named(h, vars)
        }
        return if (flatNullable(j) && h !is Head.Var && !base.endsWith("?")) "$base?" else base
    }

    private fun named(n: Named, vars: Set<String>): String {
        fun args(arity: Int) = if (arity == 0) "" else
            (0 until arity).joinToString(", ", "<", ">") { i -> n.args.getOrNull(i)?.let { jsRef(it, vars) } ?: anyN }
        return when (val d = declared(n)) {
            is Cls -> T(d) + args(d.tparams.size)
            is KindDecl -> T(d)
            else -> {
                val (mod, name) = ir.resolve(n)
                when {
                    mod != null -> "kotlin.js.JsAny"
                    name == "Promise" -> "kotlin.js.Promise${args(1)}"
                    name == "Array" -> "kotlin.js.JsArray${args(1)}"
                    else -> "kotlin.js.JsAny"
                }
            }
        }
    }

    /** A Kotlin function type for a callback parameter, which Kotlin converts to a JS function at the external. */
    private fun callbackType(h: Head.Callback): String {
        fun t(x: Type): String = when (x) {
            Type.Void -> "kotlin.Unit"
            Type.F64 -> "kotlin.Double"
            Type.I64 -> "kotlin.Long"
            Type.Bool -> "kotlin.Boolean"
            is Type.Js -> if (x.t.head is Head.Callback) (if (x.t.empty) anyN else "kotlin.js.JsAny") else jsRef(x.t, emptySet())
        }
        return "(${h.params.joinToString(", ") { t(it) }}) -> ${t(h.result)}"
    }

    private fun flatType(t: Type, param: Boolean): String = when (t) {
        Type.Void -> "kotlin.Unit"
        // A Kotlin `Boolean` parameter of an external is passed as a JS
        // boolean (`externref`), so a wasm `bool` parameter is an `Int`.
        Type.Bool -> if (param) "kotlin.Int" else "kotlin.Boolean"
        Type.F64 -> "kotlin.Double"
        Type.I64 -> "kotlin.Long"
        is Type.Js -> {
            val h = t.t.head
            if (param && h is Head.Callback) callbackType(h).let { if (t.t.empty) "($it)?" else it }
            else if (flatNullable(t.t)) anyN else "kotlin.js.JsAny"
        }
    }

    /** A position as the typed layer presents it. [const] is the flat argument for a parameter the typed layer drops. */
    private class TT(val type: String, val toFlat: (String) -> String, val fromFlat: (String) -> String, val const: String? = null) {
        /** Overload key: Kotlin's clash rules ignore nullability here. */
        val erased get() = type.removeSuffix("?")
    }

    /** [arg]: a parameter (literals and empties are dropped, callbacks are Kotlin functions); else a result. */
    private fun top(t: Type, vars: Set<String>, arg: Boolean): TT {
        val id = { x: String -> x }
        val j = when (t) {
            Type.Void -> return TT("kotlin.Unit", id, id)
            Type.F64 -> return TT("kotlin.Double", id, id)
            Type.I64 -> return TT("kotlin.Long", id, id)
            Type.Bool -> return TT("kotlin.Boolean", { "(if ($it) 1 else 0)" }, id)
            is Type.Js -> t.t
        }
        val nn = flatNullable(j)
        val n = if (nn) "?" else ""
        val dot = if (nn) "?." else "."
        fun conv(kt: String, js: String, to: String, from: String) =
            TT("$kt$n", { "$it$dot$to()" }, { "$it${dot}unsafeCast<$js>()$dot$from()" })
        val h = j.head
        // Kotlin cannot produce `undefined` (FINDINGS.md): an empty is passed as `null`.
        if (arg && !j.empty) when (h) {
            is Head.StrLit -> return TT("", id, id, "${str(h.value)}.toJsString()")
            is Head.IntLit -> return TT("", id, id, "(${h.value}.0).toJsNumber()")
            else -> {}
        }
        if (arg && h == Head.Empty) return TT("", id, id, "null")
        return when (h) {
            Head.Str, is Head.StrLit -> conv("kotlin.String", "kotlin.js.JsString", "toJsString", "toString")
            Head.Num, is Head.IntLit -> conv("kotlin.Double", "kotlin.js.JsNumber", "toJsNumber", "toDouble")
            Head.Boolean -> conv("kotlin.Boolean", "kotlin.js.JsBoolean", "toJsBoolean", "toBoolean")
            is Head.Callback -> if (arg) TT(callbackType(h).let { if (j.empty) "($it)?" else it }, id, id) else TT(jsRef(j, vars), id, id)
            else -> {
                val k = jsRef(j, vars)
                TT(k, id, { cast(it, k, vars) })
            }
        }
    }

    /** [x], a `JsAny?`, as [type]. `unsafeCast` wants a non-null `JsAny` type; a type parameter takes an unchecked `as`. */
    private fun cast(x: String, type: String, vars: Set<String> = emptySet()): String {
        val t = type.removeSuffix("?")
        return when {
            type == "kotlin.js.JsAny" || type == anyN -> x
            t in vars -> "($x as $type)"
            type.endsWith("?") -> "$x?.unsafeCast<$t>()"
            else -> "$x.unsafeCast<$t>()"
        }
    }

    private fun str(s: String) = "\"" + s.flatMap {
        when (it) {
            '\\' -> "\\\\".toList(); '"' -> "\\\"".toList(); '$' -> "\\$".toList()
            '\n' -> "\\n".toList(); '\r' -> "\\r".toList(); '\t' -> "\\t".toList()
            else -> listOf(it)
        }
    }.joinToString("") + "\""

    /** Relative path -> source, one file per package. */
    fun flatFiles(): Map<String, String> {
        val byPkg = bound.groupBy { it.import.substringBeforeLast('.', "") }
        val discsByPkg = discs.groupBy { it.import.substringBeforeLast('.', "") }
        return (byPkg.keys + discsByPkg.keys).associateWith { byPkg[it].orEmpty() }.entries.associate { (p, bs) ->
            val sb = StringBuilder()
            sb.append("// Generated by the ts-gen Kotlin generator from the IR. Do not edit.\n")
            sb.append("// Each external's fully qualified name is its import name, so it is\n")
            sb.append("// imported as \"js_code\" \"<name>\" and linked by `ts-gen embed --bindings-module js_code`.\n")
            sb.append("@file:OptIn(kotlin.js.ExperimentalWasmJsInterop::class)\n")
            sb.append("@file:Suppress(\"unused\", \"FunctionName\", \"PackageName\", \"PackageDirectoryMismatch\", \"ObjectPropertyName\")\n")
            if (p.isNotEmpty()) sb.append("package ${p.split('.').joinToString(".") { q(it) }}\n")
            sb.append('\n')
            for (b in bs) {
                val ps = mutableListOf<String>()
                val seen = mutableSetOf<String>()
                fun pname(n: String, i: Int): String { var s = sanitize(n); if (!seen.add(s)) { s = "p$i"; seen += s }; return q(s) }
                if (b.m.kind.receiver) ps += "${pname("self", -1)}: kotlin.js.JsAny"
                b.m.params.forEachIndexed { i, p -> ps += "${pname(p.name, i)}: ${flatType(p.type, true)}" }
                val msg = b.import.replace("$", "\${'$'}")
                sb.append("@JsFun(\"() => { throw new Error('$msg: not linked by ts-gen embed') }\")\n")
                sb.append("external fun ${q(b.m.name)}(${ps.joinToString(", ")}): ${flatType(b.m.result, false)}\n\n")
            }
            for (d in discsByPkg[p].orEmpty()) {
                sb.append("@JsFun(\"() => { throw new Error('${d.import}: not linked by ts-gen embed') }\")\n")
                sb.append("external fun ${q(d.import.substringAfterLast('.'))}(x: kotlin.js.JsAny?): kotlin.Int\n\n")
            }
            "flat/${p.ifEmpty { "_root" }}.kt" to sb.toString()
        }
    }

    private enum class K { PROP, FUN, FACTORY }

    /** A typed-layer declaration: an extension (with [receiver]), a factory or a holder member. */
    private inner class Fn(
        val call: String,
        val doc: String?,
        val kind: K,
        val receiver: String?,
        val params: List<Pair<String, TT>>,
        val result: TT,
        val base: String,
        val fallback: String,
        val owner: String? = null,
    ) {
        var name = ""
        val key get() = if (kind == K.PROP) "val $name" else "fun $name(${params.filter { it.second.const == null }.joinToString(",") { it.second.erased }})"

        fun render(indent: String): String {
            val sb = StringBuilder(kdoc(indent, doc))
            val args = mutableListOf<String>()
            if (receiver != null) args += "this"
            params.forEach { (n, t) -> args += t.const ?: t.toFlat(q(n)) }
            val body = result.fromFlat("$call(${args.joinToString(", ")})")
            val ps = params.filter { it.second.const == null }.joinToString(", ") { (n, t) -> "${q(n)}: ${t.type}" }
            val types = params.map { it.second.type } + result.type + (receiver ?: "")
            val tvs = tvars.filter { v -> types.any { Regex("\\b${Regex.escape(v)}\\b").containsMatchIn(it) } }
            val tp = if (tvs.isEmpty()) "" else tvs.joinToString(", ", "<", "> ") { "$it : kotlin.js.JsAny?" }
            val recv = receiver?.let { "$it." } ?: ""
            if (kind == K.PROP) sb.append("${indent}val $tp$recv${q(name)}: ${result.type}\n$indent    get() = $body\n")
            else sb.append("${indent}fun $tp$recv${q(name)}($ps): ${result.type} = $body\n")
            return sb.toString()
        }

        var tvars: List<String> = emptyList()
    }

    private fun fnFor(b: Bound): Pair<String, Fn> {
        val c = b.owner
        val m = b.m
        val ctps = c?.tparams.orEmpty()
        val prop = (m.kind == MemberKind.GET || m.kind == MemberKind.STATIC_GET) && m.params.isEmpty()
        // A property's type parameters must appear in its receiver.
        val vars = when {
            prop && m.kind.receiver -> ctps.toSet()
            prop -> emptySet()
            else -> (ctps + m.tparams).toSet()
        }
        val params = m.params.map { p -> sanitize(p.name) to top(p.type, vars, true) }.let { ps ->
            val seen = mutableSetOf<String>()
            ps.mapIndexed { i, (n, t) -> (if (seen.add(n)) n else "p$i".also { seen += it }) to t }
        }
        val res = top(m.result, vars, false)
        val receiver = if (m.kind.receiver) T(c!!) + (if (ctps.isEmpty()) "" else ctps.joinToString(", ", "<", ">")) else null
        val bind = sanitize(m.name)
        // A cohort arm, or a JS name that is no identifier (`content-type`), takes the binding name.
        val jsOk = m.cohort == null && isPlainIdent(m.js)
        val derived = sanitize(camel(bind))
        val (kind, base, fallback) = when (m.kind) {
            MemberKind.CONSTRUCTOR, MemberKind.OBJECT -> {
                val kt = ktNames.getValue(c!!)
                Triple(K.FACTORY, kt, "${kt}_${if (verbatim) bind else derived}")
            }
            MemberKind.SET, MemberKind.STATIC_SET -> Triple(K.FUN,
                if (verbatim) bind else if (jsOk) "set" + m.js.replaceFirstChar(Char::uppercase) else derived, if (verbatim) bind + "_" else derived)
            else -> Triple(if (prop) K.PROP else K.FUN, if (verbatim) bind else if (jsOk) m.js else derived, if (verbatim) bind + "_" else derived)
        }
        val scope = when {
            c == null || kind == K.FACTORY -> ""
            m.kind.receiver -> "instance ${c.name}"
            else -> "static ${c.name}"
        }
        val fn = Fn(b.alias, m.doc, kind, receiver, params, res, base, fallback, c?.name)
        fn.tvars = vars.toList()
        return scope to fn
    }

    private class ArmFn(val k: KindDecl, val arm: Arm, val kt: String, val param: TT?, val const: String?) {
        var name = ""
        var held = false
    }

    private fun nameAll(fns: List<Fn>, taken: MutableSet<String>) {
        for (f in fns) {
            f.name = f.base
            if (f.key in taken) f.name = f.fallback
            while (f.key in taken) f.name += "_"
            taken += f.key
        }
    }

    fun typedFile(): String {
        val fns = bound.map(::fnFor)
        val byScope = fns.groupBy({ it.first }, { it.second })
        val reservedInstance = setOf("fun toString()", "fun hashCode()", "fun equals(kotlin.js.JsAny)")
        for ((s, l) in byScope) if (s.startsWith("instance ")) nameAll(l, reservedInstance.toMutableSet())
        for ((s, l) in byScope) if (s.startsWith("static ")) nameAll(l, mutableSetOf())

        // Kinds: class arms become supertypes; other arms factories, or holder members.
        val supers = mutableMapOf<Any, MutableList<Pair<Any, String>>>() // Cls or KindDecl -> (supertype, Kotlin type)
        for (c in classes.values) if (c.kind != ClassKind.STATIC) supers[c] = mutableListOf()
        for (k in kinds.values) supers[k] = mutableListOf()
        fun reaches(from: Any, to: Any, seen: MutableSet<Any> = mutableSetOf()): Boolean =
            from === to || (seen.add(from) && supers[from].orEmpty().any { reaches(it.first, to, seen) })
        fun addSuper(sub: Any, sup: Any, type: String): Boolean {
            val l = supers[sub] ?: return false
            if (l.any { it.first === sup }) return l.any { it.second == type }
            if (reaches(sup, sub)) return false
            l += sup to type
            return true
        }
        for (c in classes.values) if (c.kind != ClassKind.STATIC) for (e in c.extends) {
            val d = declared(e) as? Cls ?: continue
            addSuper(c, d, named(e, c.tparams.toSet()))
        }
        val armFns = mutableListOf<ArmFn>()
        for (k in kinds.values) for (a in k.arms) {
            val h = a.type.head
            val d = (h as? Named)?.let(::declared)
            if (d != null && addSuper(d, k, T(k))) continue
            val kt = T(k)
            if (h is Head.Callback) continue
            val t = top(Type.Js(a.type), emptySet(), true)
            armFns += ArmFn(k, a, kt, t.takeIf { it.const == null }, t.const)
        }

        // The package scope: factories, kind factories and top-level members.
        val objects = mutableSetOf<String>()
        classes.values.filter { it.kind == ClassKind.STATIC }.forEach { objects += ktNames.getValue(it) }
        objects += holders.values
        // A top-level property may not share a classifier's name (`const Cloudflare: Cloudflare`).
        val topTaken = (objects + classifiers).map { "val $it" }.toMutableSet()
        val pkgFns = byScope[""].orEmpty()
        nameAll(pkgFns.filter { it.kind == K.FACTORY }, topTaken)
        // An arm is a factory overload named like its kind, else (a literal, or
        // a clash) a member of the kind's holder named like the arm.
        for (f in armFns) if (f.param != null) {
            val key = "fun ${ktNames.getValue(f.k)}(${f.param.erased})"
            if (topTaken.add(key)) f.name = ktNames.getValue(f.k)
        }
        nameAll(pkgFns.filter { it.kind != K.FACTORY }, topTaken)
        for ((k, l) in armFns.filter { it.name.isEmpty() }.groupBy { it.k }) {
            holders[k] = holderName(k)
            val taken = mutableSetOf<String>()
            for (f in l) {
                f.name = sanitize(f.arm.name)
                fun key() = if (f.param == null) "val ${f.name}" else "fun ${f.name}(${f.param.erased})"
                while (key() in taken) f.name += "_"
                taken += key()
                f.held = true
            }
        }

        fun ancestors(c: Cls, acc: MutableSet<Cls> = linkedSetOf()): Set<Cls> {
            for ((d, _) in supers[c].orEmpty()) if (d is Cls && acc.add(d)) ancestors(d, acc)
            return acc
        }
        // A member two unrelated bases declare is ambiguous on the derived
        // class (neither extension's receiver is more specific): forward it
        // to the first. Generic classes are left out.
        fun forwarders(c: Cls): List<String> {
            if (c.tparams.isNotEmpty()) return emptyList()
            val own = byScope["instance ${c.name}"].orEmpty().map { it.key }.toSet()
            val decls = linkedMapOf<String, MutableList<Pair<Cls, Fn>>>()
            val anc = ancestors(c)
            for (d in anc) if (d.tparams.isEmpty()) for (f in byScope["instance ${d.name}"].orEmpty())
                if (f.key !in own && f.tvars.isEmpty()) decls.getOrPut(f.key) { mutableListOf() } += d to f
            return decls.values.mapNotNull { l ->
                val (d, f) = l.filter { (d, _) -> l.none { (e, _) -> e !== d && d in ancestors(e) } }.takeIf { it.size > 1 }?.first() ?: return@mapNotNull null
                val ps = f.params.filter { it.second.const == null }
                val via = "(this as ${T(d)}).${q(f.name)}"
                if (f.kind == K.PROP) "val ${T(c)}.${q(f.name)}: ${f.result.type}\n    get() = $via\n"
                else "fun ${T(c)}.${q(f.name)}(${ps.joinToString(", ") { (n, t) -> "${q(n)}: ${t.type}" }}): ${f.result.type} = $via(${ps.joinToString(", ") { q(it.first) }})\n"
            }
        }
        val factories = fns.filter { it.second.kind == K.FACTORY }.groupBy({ it.second.owner }, { it.second })

        val sb = StringBuilder()
        sb.append("// Generated by the ts-gen Kotlin generator from the IR. Do not edit.\n")
        sb.append("@file:OptIn(kotlin.js.ExperimentalWasmJsInterop::class)\n")
        sb.append("@file:Suppress(\"unused\", \"FunctionName\", \"PropertyName\", \"ObjectPropertyName\", \"ClassName\", \"RedundantSuppression\", \"UNCHECKED_CAST\", \"EXTENSION_SHADOWED_BY_MEMBER\", \"NOTHING_TO_INLINE\")\n")
        sb.append("package $pkg\n\n")
        sb.append("import kotlinx.coroutines.await as kotlinxAwait\n")
        for (b in bound) sb.append("import ${b.import.split('.').joinToString(".") { q(it) }} as ${b.alias}\n")
        for (d in discs) sb.append("import ${d.import.split('.').joinToString(".") { q(it) }} as ${d.alias}\n")
        if (skipped.isNotEmpty()) {
            sb.append("\n// Not bound (module-scoped and variadic bindings are out of scope):\n")
            skipped.forEach { sb.append("//   $it\n") }
        }
        sb.append("\n/** Awaits a typed promise: kotlinx's `await` on wasm leaves the result type to the caller. */\n")
        sb.append("suspend fun <T : kotlin.js.JsAny?> kotlin.js.Promise<T>.await(): T = kotlinxAwait<T>()\n\n")

        fun arm(f: ArmFn, indent: String) {
            sb.append(kdoc(indent, f.arm.doc))
            // Only reinterprets the value: `unsafeCast` to the kind.
            if (f.param == null) sb.append("${indent}val ${q(f.name)}: ${f.kt}\n$indent    get() = ${cast(f.const!!, f.kt)}\n")
            else sb.append("${indent}fun ${q(f.name)}(value: ${f.param.type}): ${f.kt} = ${cast(f.param.toFlat("value"), f.kt)}\n")
        }

        for (c in classes.values) {
            val kt = ktNames.getValue(c)
            if (c.kind == ClassKind.STATIC) {
                sb.append(kdoc("", c.doc))
                sb.append("object ${q(kt)} {\n")
                byScope["static ${c.name}"].orEmpty().forEach { sb.append(it.render("    ")) }
                sb.append("}\n\n")
                continue
            }
            val tps = if (c.tparams.isEmpty()) "" else c.tparams.joinToString(", ", "<", ">") { "$it : kotlin.js.JsAny?" }
            val sup = supers.getValue(c).map { it.second }.ifEmpty { listOf("kotlin.js.JsAny") }
            sb.append(kdoc("", c.doc))
            sb.append("external interface ${q(kt)}$tps : ${sup.joinToString(", ")}\n\n")
            byScope["instance ${c.name}"].orEmpty().forEach { sb.append(it.render("")) }
            forwarders(c).forEach { sb.append(it) }
            factories[c.name].orEmpty().forEach { sb.append(it.render("")) }
            holders[c]?.let { h ->
                sb.append("\n/** Statics of [$kt]. */\nobject ${q(h)} {\n")
                byScope["static ${c.name}"].orEmpty().forEach { sb.append(it.render("    ")) }
                sb.append("}\n")
            }
            sb.append('\n')
        }

        for (k in kinds.values) {
            val kt = ktNames.getValue(k)
            val sup = supers.getValue(k).map { it.second }.ifEmpty { listOf("kotlin.js.JsAny") }
            val arms = k.arms.joinToString("\n") { a -> "* `${a.name}`: ${irType(a.type)}" }
            val union = if (k.arms.isEmpty()) null else "A union${if (k.fallback) " (open)" else ""} of:\n$arms"
            sb.append(kdoc("", listOfNotNull(k.doc, union).joinToString("\n\n").ifEmpty { null }))
            sb.append("external interface ${q(kt)} : ${sup.joinToString(", ")}\n")
            armFns.filter { it.k === k && !it.held }.forEach { arm(it, "") }
            holders[k]?.let { h ->
                sb.append("\n/** Arms of [$kt] that are not its factories' overloads. */\nobject ${q(h)} {\n")
                armFns.filter { it.k === k && it.held }.forEach { arm(it, "    ") }
                sb.append("}\n")
            }
            discs.find { it.k === k }?.let { matchArms(sb, it) }
            sb.append('\n')
        }

        for (c in classes.values) if (c.kind != ClassKind.STATIC) for (a in c.aliases) {
            if (!isPlainIdent(a) || '.' in a || !classifiers.add(a)) continue
            val tps = if (c.tparams.isEmpty()) "" else c.tparams.joinToString(", ", "<", ">")
            sb.append("typealias ${q(a)}$tps = ${T(c)}$tps\n")
        }
        sb.append('\n')

        pkgFns.filter { it.kind != K.FACTORY }.forEach { sb.append(it.render("")).append('\n') }
        return sb.toString()
    }

    /**
     * A kind's arms as a sealed interface, a case per arm holding the value
     * as a result of its type would be (a literal arm an object), and `arm`,
     * which tells the case with the discriminator (IR.md, Recognition). In
     * a `fallback` kind the last non-literal arm takes what no other does;
     * otherwise a value of no arm throws, as Rust's binding does.
     */
    private fun matchArms(sb: StringBuilder, d: Disc) {
        val k = d.k
        val kt = T(k)
        val at = armTypes.getValue(k)
        val names = mutableSetOf<String>()
        val cases = k.arms.map { a ->
            var n = sanitize(a.name)
            while (!names.add(n)) n += "_"
            n
        }
        sb.append("\n/** The arm a [${ktNames.getValue(k)}] holds, from [arm]. */\n")
        sb.append("sealed interface ${q(at)} {\n")
        val build = k.arms.mapIndexed { i, a ->
            val case = q(cases[i])
            sb.append(kdoc("    ", a.doc))
            when (a.type.head) {
                is Head.StrLit, is Head.IntLit -> {
                    sb.append("    data object $case : ${q(at)}\n")
                    "${q(at)}.$case"
                }
                else -> {
                    val t = top(Type.Js(a.type), emptySet(), false)
                    sb.append("    class $case(val value: ${t.type}) : ${q(at)}\n")
                    "${q(at)}.$case(${t.fromFlat("this")})"
                }
            }
        }
        sb.append("}\n\n")
        val fallback = if (k.fallback) k.arms.indexOfLast { it.type.head !is Head.StrLit && it.type.head !is Head.IntLit }.takeIf { it >= 0 } else null
        sb.append("/** The arm this value holds. */\n")
        sb.append("val $kt.arm: $pkg.${q(at)}\n    get() = when (${d.alias}(this)) {\n")
        build.forEachIndexed { i, b -> if (i != fallback) sb.append("        $i -> $b\n") }
        sb.append("        else -> ${fallback?.let { build[it] } ?: "throw IllegalStateException(${str("no arm of ${k.path}")})"}\n    }\n")
    }

    /** [t] as the IR writes it, for docs. */
    private fun irType(t: JsType): String {
        val h = when (val h = t.head) {
            Head.Any -> "Any"; Head.Str -> "String"; Head.Num -> "Number"; Head.Boolean -> "Boolean"; Head.BigInt -> "BigInt"
            Head.Empty -> null
            is Head.Var -> h.name
            is Head.StrLit -> str(h.value)
            is Head.IntLit -> h.value.toString()
            is Head.Tuple -> h.elems.joinToString(", ", "[", "]") { irType(it) }
            is Head.Callback -> "(callback)"
            is Named -> h.name + if (h.args.isEmpty()) "" else h.args.joinToString(", ", "<", ">") { irType(it) }
        }
        return listOfNotNull(h, "Null".takeIf { t.nul }, "Undefined".takeIf { t.undef }).joinToString(" | ").replace("`", "'")
    }

    fun entryFile(): String? {
        if (ir.provides.isEmpty()) return null
        val sb = StringBuilder()
        sb.append("// Generated by the ts-gen Kotlin generator from the IR. Do not edit.\n")
        sb.append("// One export per provide: unwraps the wire values and calls `$appPkg.<name>`.\n")
        sb.append("@file:OptIn(kotlin.js.ExperimentalWasmJsInterop::class, kotlinx.coroutines.DelicateCoroutinesApi::class)\n")
        sb.append("package $pkg.provided\n\n")
        sb.append("import kotlinx.coroutines.CoroutineStart\nimport kotlinx.coroutines.GlobalScope\nimport kotlinx.coroutines.promise\n\n")
        for (h in ir.provides) {
            val ps = h.params.mapIndexed { i, p -> (sanitize(p.name).ifEmpty { "p$i" }) to p.type }
            val wire = ps.joinToString(", ") { (n, t) ->
                // A `Boolean` export parameter is an i32, as the wire wants.
                "${q(n)}: ${if (t == Type.Bool) "kotlin.Boolean" else flatType(t, false)}"
            }
            val args = ps.joinToString(", ") { (n, t) -> top(t, emptySet(), false).fromFlat(q(n)) }
            val call = "$appPkg.${appName(h)}($args)"
            val res = top(h.result, emptySet(), false)
            sb.append(kdoc("", h.doc))
            // The export name may be dotted (`api.math.add`); @JsName sets it.
            sb.append("@JsExport\n@JsName(${str(h.name)})\n")
            if (h.async) {
                // Not JSPI: the export returns the promise, which `promise {}` makes.
                val v = when (h.result) {
                    Type.Void -> "$call.let { null }"
                    Type.Bool -> "$call.toJsBoolean()"
                    Type.F64 -> "$call.toJsNumber()"
                    Type.I64 -> "$call.toJsBigInt()"
                    is Type.Js -> res.toFlat(call)
                }
                sb.append("fun ${appName(h)}($wire): kotlin.js.JsAny =\n")
                sb.append("    GlobalScope.promise(start = CoroutineStart.UNDISPATCHED) { $v }\n\n")
            } else {
                val (v, wr) = when (h.result) {
                    // A `Boolean` export result would be a JS boolean.
                    Type.Bool -> "(if ($call) 1 else 0)" to "kotlin.Int"
                    else -> res.toFlat(call) to flatType(h.result, false)
                }
                sb.append("fun ${appName(h)}($wire): $wr = $v\n\n")
            }
        }
        return sb.toString()
    }

    /** The user functions the entry calls, for a scaffold. */
    fun appSignatures(): String = ir.provides.joinToString("\n") { h ->
        val ps = h.params.map { p -> "${q(sanitize(p.name))}: ${top(p.type, emptySet(), false).type}" }
        val res = if (h.result == Type.Void) "Unit" else top(h.result, emptySet(), false).type
        (if (h.async) "suspend " else "") + "fun ${appName(h)}(${ps.joinToString(", ")}): $res"
    }

    /** The user function a provide calls: its export name, `.` as `_` (`api_math_add`). */
    private fun appName(h: Provide) = q(sanitize(h.name.replace('.', '_')))
}
