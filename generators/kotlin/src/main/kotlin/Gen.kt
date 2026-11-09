package tsgen.kotlin

/** A position Kotlin can't express under this profile; the member is skipped. */
class Unsupported(message: String) : Exception(message)

private fun unsupported(why: String): Nothing = throw Unsupported(why)

/** Wire values ready to pass: statements to run first, then one expression per value. */
private class Conv(val pre: List<String>, val exprs: List<String>)

/** Names the generated code uses unqualified, which a declaration must not shadow. */
private val reserved = setOf(
    "String", "Int", "Boolean", "Double", "Float", "Long", "Short", "Byte", "UInt", "ULong",
    "UByte", "UShort", "Unit", "Any", "Nothing", "Pair", "Triple", "JsAny", "VecU8", "VecI8", "VecU16", "VecI16", "VecU32", "VecI32", "VecI64", "VecU64", "VecF16", "VecF32", "VecF64",
    "JsString", "JsNumber", "JsBoolean", "JsArray", "JsReference",
)

/**
 * Kotlin/Wasm bindings for a lowered IR under
 * `all-host, results = single, throws = js` (see [checkProfile]).
 *
 * Two layers: `<pkg>.flat` holds one external or export per member, typed
 * by its core wire; `<pkg>` holds the typed API (external interfaces with
 * extension members, enums) calling it. Kotlin imports only from `js_code`
 * the functions it can type (`@WasmImport` takes no references, and its
 * modules become ESM imports of the glue), so every import is a `@JsFun`
 * listed in [links], to be renamed to its key in the compiled module.
 */
class Gen(
    private val low: Lowered,
    private val pkg: String,
    private val appPkg: String,
    private val libPkgs: Map<String, String> = emptyMap(),
) {
    private val flatPkg = "$pkg.flat"
    private val classes = LinkedHashMap<String, DeclL>()
    private val names = HashMap<String, String>()

    /** A lib declaration's path, as its lib's bindings name it (`std.Date`). */
    private val libNames = HashMap<String, String>()
    private val enums = LinkedHashMap<String, EnumL>()
    private val flatNames = HashSet<String>()
    private val trampolines = LinkedHashMap<String, Repr>()
    /** The `fun interface`s of recursive function types used, by spelling. */
    private val funInterfaces = LinkedHashMap<String, RecL>()
    private val calls = LinkedHashMap<String, String>()
    private val flatOut = StringBuilder()
    private val exportOut = StringBuilder()
    private val appOut = StringBuilder()
    private val appClasses = LinkedHashMap<String, MutableList<String>>()
    private var tmp = 0

    /** `js_code` import names of the `@JsFun` externals, with the import each one is. */
    val links = mutableListOf<Pair<String, Import>>()

    /** Members left out, with the reason. */
    val skipped = mutableListOf<String>()

    /** Approximations, by the member or declaration they're in. */
    private val warnings = LinkedHashSet<String>()
    private var at = ""

    init {
        names += declNames(low.decls)
        for (d in low.decls) if (d.kind == "class" || d.kind == "interface") classes[d.path] = d
        for (lib in low.libs) {
            val lp = libPkgs.getValue(lib.name)
            for ((path, n) in declNames(lib.decls)) if (path !in names) libNames.putIfAbsent(path, "$lp.$n")
            for (d in lib.decls) if (d.kind == "class" || d.kind == "interface") classes.putIfAbsent(d.path, d)
        }
        for (d in low.decls) if (d.kind == "enum" && d.enum != null) enums[d.enum.path] = d.enum
    }

    /** Kotlin names for declarations: simple, or by path where simple names collide. */
    private fun declNames(decls: List<DeclL>): Map<String, String> {
        val typed = decls.filter { it.kind in setOf("class", "interface", "enum", "alias") }
        val count = typed.groupingBy { sanitize(it.name) }.eachCount()
        return typed.associate { d ->
            val simple = sanitize(d.name)
            val n = if (count[simple] == 1) simple else sanitize(d.path)
            d.path to if (n in reserved) "${n}_" else n
        }
    }

    private fun typeName(path: String) = names[path] ?: libNames[path] ?: sanitize(path)

    /** A type reference: lib types are already package-qualified. */
    private fun typeRef(path: String) = if (path in libNames) typeName(path) else qual + typeName(path)

    /**
     * The package prefix for type references, set where nested names can
     * shadow them: an enum arm named like its payload's type.
     */
    private var qual = ""

    /** Erased type parameters as `JsAny?`, outside any generic scope. */
    private var eraseToAny = false

    /** A host class as a type: its type arguments erased to their defaults, else `JsAny?`. */
    private fun hostType(path: String) = typeRef(path) +
        (classes[path]?.tparams?.takeIf { it.isNotEmpty() }?.joinToString(prefix = "<", postfix = ">") { erased(it) } ?: "")

    private val erasing = mutableSetOf<TParam>()

    /** A type parameter's erasure: its default when that's a JS reference, else `JsAny?`. */
    private fun erased(p: TParam): String {
        val d = p.default ?: return "JsAny?"
        if (d.kind != "host" || d.of == "future" || !erasing.add(p)) return "JsAny?"
        try {
            return kt(d)
        } finally {
            erasing.remove(p)
        }
    }

    private fun fresh(p: String) = "$p${tmp++}"

    private fun opt(r: Repr) = if (r.nullable) "?" else ""

    private fun scalarKt(s: String) = when (s) {
        "bool" -> "Boolean"
        "i8" -> "Byte"
        "u8" -> "UByte"
        "i16" -> "Short"
        "u16" -> "UShort"
        "i32" -> "Int"
        "u32" -> "UInt"
        "i53", "u53", "i64" -> "Long"
        "u64" -> "ULong"
        "f16", "f32" -> "Float"
        "f64" -> "Double"
        else -> unsupported("scalar $s")
    }

    private fun sTo(s: String, e: String) = when (s) {
        "bool" -> "(if ($e) 1 else 0)"
        "i8", "u8", "i16", "u16", "u32" -> "$e.toInt()"
        "u64" -> "$e.toLong()"
        else -> e
    }

    /** `e?.let { <to wire> }`, or just `e` when the conversion is the identity. */
    private fun orElse(e: String, s: String) = sTo(s, "it").let { if (it == "it") e else "$e?.let { $it }" }

    private fun sFrom(s: String, w: String) = when (s) {
        "bool" -> "($w != 0)"
        "i8" -> "$w.toByte()"
        "u8" -> "$w.toUByte()"
        "i16" -> "$w.toShort()"
        "u16" -> "$w.toUShort()"
        "u32" -> "$w.toUInt()"
        "u64" -> "$w.toULong()"
        else -> w
    }

    private fun sLit(s: String, n: Long) = when (s) {
        "i8" -> "($n).toByte()"
        "u8" -> "${n}u.toUByte()"
        "i16" -> "($n).toShort()"
        "u16" -> "${n}u.toUShort()"
        "i32" -> "$n"
        "u32" -> "${n}u"
        "i64" -> "${n}L"
        "u64" -> "${n}uL"
        else -> unsupported("carrier $s")
    }

    private fun wireKt(w: String) = when (w) {
        "i32" -> "Int"
        "i64" -> "Long"
        "f32" -> "Float"
        "f64" -> "Double"
        "externref" -> "JsAny?"
        else -> unsupported("wire type $w (Kotlin externals take only primitives and externref)")
    }

    private fun zero(w: String) = when (w) {
        "i32" -> "0"
        "i64" -> "0L"
        "f32" -> "0f"
        "f64" -> "0.0"
        else -> "null"
    }

    private fun niche(n: Long, w: String) = when {
        w == "i32" && n == Int.MIN_VALUE.toLong() -> "Int.MIN_VALUE"
        w == "i64" && n == Long.MIN_VALUE -> "Long.MIN_VALUE"
        w == "i64" -> "${n}L"
        else -> "$n"
    }

    private fun erasedName(r: Repr) = sanitize(r.ty.removeSuffix("?"))

    private fun enumKt(e: EnumL): String {
        if (e.path in libNames) return if (e.origin == "nested") kt(undefinedValue(e)).removeSuffix("?") + "?" else typeName(e.path)
        enums.putIfAbsent(e.path, e)
        if (e.origin == "nested") return kt(undefinedValue(e)).removeSuffix("?") + "?"
        return qual + if (e.origin == "err") "Err" else typeName(e.path)
    }

    private fun undefinedValue(e: EnumL) = e.arms.first { it.name == "Value" }.payload[0]

    private fun appClass(path: String) = "$appPkg.${typeName(path)}"

    /** The JS-level type of an `arr<T>` element, as a `JsArray` argument. */
    private fun jsElem(r: Repr): String = when (r.kind) {
        "str", "scalar" -> boxKt(r)
        "host" -> when (r.of) {
            "class", "interface" -> hostType(r.path)
            "box" -> boxKt(r.inner)
            else -> "JsAny"
        }
        else -> "JsAny"
    } + "?"

    private fun boxKt(inner: Repr) = when (inner.kind) {
        "str" -> "JsString"
        "bigint" -> "JsBigInt"
        "scalar" -> when (inner.scalar) {
            "bool" -> "JsBoolean"
            "i64", "u64" -> "JsAny"
            else -> "JsNumber"
        }
        else -> "JsAny"
    }

    /**
     * The `fun interface` a function type a recursive alias reaches again
     * declares: Kotlin's function types can't name themselves.
     */
    private fun recOf(r: Repr): RecL? =
        if (r.kind == "fn" || r.kind == "rec") low.recs[r.ty.removeSuffix("?")] else null

    private fun funInterface(rec: RecL): String {
        funInterfaces.putIfAbsent(rec.ty, rec)
        return sanitize(rec.name)
    }

    /** A `rec` as the function layout it stands for. */
    private fun unrolled(r: Repr): Repr {
        val rec = low.recs[r.ty.removeSuffix("?")] ?: unsupported("`${r.ty}` (rec) with no layout")
        val base = if (r.provided != null) rec.toJs else rec.toLang
        return Repr(base.ty, base.wire, base.kind, r.nullable, base.m)
    }

    /** The Kotlin type of a value. */
    private fun kt(r: Repr): String = recOf(r)?.let { funInterface(it) + opt(r) } ?: when (r.kind) {
        "unit" -> "Unit"
        // The bridge throws if a `never` call returns.
        "never" -> "Nothing"
        "scalar" -> scalarKt(r.scalar) + opt(r)
        "str" -> "String" + opt(r)
        "vec" -> vecKt(r.vecElem) + opt(r)
        // The host carrier's bigint is the JS bigint itself.
        "bigint" -> if (r.carrier == "host") "JsBigInt" + opt(r) else unsupported("`bigint` on the ${r.carrier} carrier")
        "array" -> "JsArray<${jsElem(r.elem)}>" + opt(r)
        "tuple" -> {
            if (r.nullable) unsupported("a nullable tuple")
            val e = r.elems.map { kt(it) }
            "${tupleClass(e.size)}<${e.joinToString()}>"
        }
        "enum" -> {
            val k = enumKt(r.enum)
            if (r.enum.origin == "nested") k else k + opt(r)
        }
        "host" -> when (r.of) {
            "class", "interface" -> hostType(r.path) + opt(r)
            "any" -> "JsAny?"
            "box" -> boxKt(r.inner) + opt(r)
            "future" -> "tsgen.runtime.Future<${kt(r.value)}>" + opt(r)
            else -> unsupported("host ${r.of}")
        }
        "handle" -> appClass(r.handleClass) + opt(r)
        "fn" -> {
            val t = "(${r.fnParams.joinToString { kt(it) }}) -> ${kt(r.fnResult)}"
            if (r.nullable) "($t)?" else t
        }
        "erased" -> if (eraseToAny) "JsAny?" else erasedName(r) + opt(r)
        else -> unsupported("`${r.ty}` (${r.kind})")
    }

    /** A Kotlin value `e` (evaluated more than once) as its wire values. */
    private fun toWire(r: Repr, e: String): Conv {
        if (r.kind == "rec") return toWire(unrolled(r), e)
        val n = r.nullable
        val one = { x: String -> Conv(emptyList(), listOf(x)) }
        return when (r.kind) {
            "scalar" -> when {
                !n -> one(sTo(r.scalar, e))
                r.niche != null -> one("(${orElse(e, r.scalar)} ?: ${niche(r.niche!!, r.wire[0])})")
                else -> Conv(emptyList(), listOf("(if ($e == null) 0 else 1)", "(${orElse(e, r.scalar)} ?: ${zero(r.wire[1])})"))
            }
            "str" -> one(if (n) "$e?.toJsString()" else "$e.toJsString()")
            "vec", "array", "erased", "bigint" -> one(e)
            "host" -> when (r.of) {
                "future" -> one(if (n) "$e?.raw" else "$e.raw")
                else -> one(e)
            }
            "tuple" -> {
                if (n) unsupported("a nullable tuple")
                val parts = r.elems.mapIndexed { i, t -> toWire(t, "$e.${tupleField(i)}") }
                Conv(parts.flatMap { it.pre }, parts.flatMap { it.exprs })
            }
            "enum" -> {
                val l = r.enum
                enumKt(l)
                when {
                    l.origin == "nested" || l.shape == "payload" -> {
                        val t = fresh("w")
                        Conv(listOf("val $t = ${encoder(l)}($e)"), listOf("$t.arm") + l.slots.indices.map { "$t.s$it" })
                    }
                    l.shape == "index" -> one(if (n) "($e?.index ?: 0)" else "$e.index")
                    else -> {
                        val c = l.carrierScalar!!
                        if (!n) one(sTo(c, "$e.value"))
                        else Conv(emptyList(), listOf("(if ($e == null) 0 else 1)", "($e?.let { ${sTo(c, "it.value")} } ?: ${zero(r.wire[1])})"))
                    }
                }
            }
            "handle" -> one(if (n) "$e?.toJsReference()" else "$e.toJsReference()")
            "fn" -> {
                if (r.provided == null) unsupported("a JS function passed back to JS")
                trampolines.putIfAbsent(r.ty.removeSuffix("?"), r)
                one(if (n) "$e?.toJsReference()" else "$e.toJsReference()")
            }
            else -> unsupported("`${r.ty}` (${r.kind}) as an argument")
        }
    }

    /** Wire values `ws` as a Kotlin value of `r`. */
    private fun fromWire(r: Repr, ws: List<String>): String {
        if (r.kind == "rec") return fromWire(unrolled(r), ws)
        val n = r.nullable
        val w = ws.firstOrNull() ?: ""
        val cast = { t: String -> if (n) "$w?.unsafeCast<$t>()" else "$w!!.unsafeCast<$t>()" }
        return when (r.kind) {
            "scalar" -> when {
                !n -> sFrom(r.scalar, w)
                r.niche != null -> "$w.let { if (it == ${niche(r.niche!!, r.wire[0])}) null else ${sFrom(r.scalar, "it")} }"
                else -> "(if ($w == 0) null else ${sFrom(r.scalar, ws[1])})"
            }
            "str" -> if (n) "$w?.unsafeCast<JsString>()?.toString()" else "$w!!.unsafeCast<JsString>().toString()"
            "vec" -> cast(vecKt(r.vecElem))
            "bigint" -> cast("JsBigInt")
            "array" -> cast(kt(r).removeSuffix("?"))
            "host" -> when (r.of) {
                "class", "interface" -> cast(hostType(r.path))
                "any" -> w
                "box" -> cast(boxKt(r.inner))
                "future" -> {
                    // A future whose `take<T>` Kotlin can't import can still be passed on or dropped.
                    val take = when {
                        r.value.kind == "unit" -> "{ f -> ${takeCall(r)}; Unit }"
                        r.value.kind == "never" -> "{ f -> ${takeCall(r)}; error(\"a never future resolved\") }"
                        else -> {
                            val pre = mutableListOf<String>()
                            val ws = spilled(takeCall(r), r.value.wire.take(1), r.value.wire.drop(1), pre)
                            "{ f -> ${(pre + fromWire(r.value, ws)).joinToString("; ")} }"
                        }
                    }
                    val rejection = "{ f -> ${errors(r.rejection).futureError(r.errorOp)}(f) }"
                    val t = "tsgen.runtime.Future<${kt(r.value)}>"
                    if (n) "$w?.let { $t(it, $take, $rejection) }" else "$t($w!!, $take, $rejection)"
                }
                else -> unsupported("host ${r.of}")
            }
            "tuple" -> {
                if (n) unsupported("a nullable tuple")
                var i = 0
                val parts = r.elems.map { t ->
                    val k = t.wire.size
                    fromWire(t, ws.subList(i, i + k)).also { i += k }
                }
                tupleClass(parts.size) + "(${parts.joinToString()})"
            }
            "enum" -> {
                val l = r.enum
                val name = enumKt(l)
                when {
                    l.origin == "nested" -> "${decoder(l)}(${ws.joinToString()})"
                    l.shape == "payload" -> "${decoder(l)}(${ws.joinToString()})" + if (n) "" else "!!"
                    l.shape == "index" -> if (n) "(if ($w == 0) null else $name.fromIndex($w))" else "$name.fromIndex($w)"
                    else -> {
                        val c = l.carrierScalar!!
                        if (n) "(if ($w == 0) null else $name(${sFrom(c, ws[1])}))" else "$name(${sFrom(c, w)})"
                    }
                }
            }
            "handle" -> {
                val t = "JsReference<${appClass(r.handleClass)}>"
                if (n) "$w?.unsafeCast<$t>()?.get()" else "$w!!.unsafeCast<$t>().get()"
            }
            "fn" -> {
                if (r.provided != null) unsupported("a language function received back")
                val call = callImport(r)
                val ps = r.fnParams.mapIndexed { i, p -> "a$i: ${kt(p)}" }
                val convs = r.fnParams.mapIndexed { i, p -> toWire(p, "a$i") }
                val args = (listOf("f") + convs.flatMap { it.exprs }).joinToString()
                val res = r.fnResult
                val callExpr = "$flatPkg.$call($args)"
                val op = r.op!!
                val pre = convs.flatMap { it.pre }.toMutableList()
                val ws = spilled(callExpr, op.coreResults, op.coreSpill, pre)
                val last = when (res.kind) {
                    "unit" -> null
                    "never" -> "error(\"a never call returned\")"
                    else -> fromWire(res, ws)
                }
                val body = (pre + listOfNotNull(last)).joinToString("; ")
                val sam = recOf(r)?.let { funInterface(it) + " " } ?: ""
                val lambda = "{ f -> $sam{ ${ps.joinToString()} -> $body } }"
                if (n) "$w?.let $lambda" else "$w!!.let $lambda"
            }
            "erased" -> "($w as ${kt(r)})"
            else -> unsupported("`${r.ty}` (${r.kind}) as a result")
        }
    }

    private fun flatName(base: String): String {
        var n = sanitize(base)
        var i = 2
        while (!flatNames.add(n)) n = sanitize(base) + "_" + i++
        return n
    }

    /** A `<pkg>.flat` external for an import, named `fname`. */
    private fun flatImport(fname: String, imp: Import, params: List<String>, results: List<String>, doc: String): String {
        val ps = params.mapIndexed { i, w -> "p$i: ${wireKt(w)}" }
        val res = results.firstOrNull()?.let { ": ${wireKt(it)}" } ?: ""
        links += "$flatPkg.$fname" to imp
        val js = "(${params.indices.joinToString { "p$it" }}) => { throw new Error('ts-gen: unlinked import') }"
        val ann = "/** ${imp.module} ${imp.name.replace("*/", "*&#47;")} */\n@JsFun(${lit(js)})"
        return "$doc$ann\nexternal fun $fname(${ps.joinToString()})$res\n"
    }

    /**
     * A language-created future for an export: `new` makes the future and
     * its resolver, and a task on the main loop settles it with `resolve<T>`
     * or `reject<E>`.
     */
    private fun futureExport(f: Repr, call: String): List<String> {
        val v = f.value
        val new = futureOp(f.futureNew, emptyList(), listOf("externref"))
        val resolve = futureOp(f.resolve, listOf("externref") + v.wire, emptyList())
        val reject = errors(f.rejection).reject(f.reject)
        val settle = if (v.kind == "unit" || v.kind == "never") "$flatPkg.$resolve(resolver)" else {
            val c = toWire(v, "r")
            (c.pre + "$flatPkg.$resolve(${(listOf("resolver") + c.exprs).joinToString()})").joinToString("; ")
        }
        return listOf(
            "val future = $flatPkg.$new()",
            "val resolver = ${pop("externref")}",
            "tsgen.runtime.Loop.main.launch({ $call }) { o -> o.fold({ r -> $settle }, { e -> $reject(resolver, e) }) }",
            "return future",
        )
    }

    /** `take<T>` of a future, on `f`; slots 1.. are left to pop. */
    private fun takeCall(r: Repr): String {
        val v = r.value
        val name = takes.getOrPut(r.take.name) {
            flatName("take_${r.value.ty}").also {
                flatOut.append(flatImport(it, r.take, listOf("externref"), v.wire.take(1), "")).append('\n')
            }
        }
        return "$flatPkg.$name(f)"
    }

    private val takes = HashMap<String, String>()

    private fun callImport(r: Repr): String {
        val ty = r.ty.removeSuffix("?")
        calls[ty]?.let { return it }
        val op = r.op ?: unsupported("no call operation for `$ty`")
        val name = flatName("call_$ty")
        flatOut.append(flatImport(name, r.call, op.coreParams, op.coreResults, "")).append('\n')
        calls[ty] = name
        return name
    }

    private fun exportAnn(name: String, wires: List<String>) =
        if (wires.none { it == "externref" }) "@WasmExport(${lit(name)})" else "@JsExport\n@JsName(${lit(name)})"

    private fun tparamDecl(names: List<String>) =
        if (names.isEmpty()) "" else "<${names.joinToString { "${sanitize(it)} : JsAny?" }}> "

    private fun ownerType(d: DeclL) =
        typeName(d.path) + if (d.tparams.isEmpty()) "" else "<${d.tparams.joinToString { sanitize(it.name) }}>"

    /** The typed body of an import: converts the parameters, calls `flat`, converts the result. */
    private fun importBody(m: MemberL, sig: Sig, fname: String, args: Map<Int, String>, receiver: String, resultType: String? = null): List<String> {
        val pre = mutableListOf<String>()
        val wires = mutableListOf<String>()
        for (p in sig.params) {
            when (p.role) {
                "this" -> wires += receiver
                "param" -> {
                    val c = toWire(p.repr!!, args.getValue(p.index!!))
                    pre += c.pre
                    wires += c.exprs
                }
                else -> unsupported("an import position `${p.role}`")
            }
        }
        val call = guarded("$flatPkg.$fname(${wires.joinToString()})", sig.throws)
        val res = sig.results.firstOrNull()?.repr
        val ws = spilled(call, sig.coreResults, sig.coreSpill, pre)
        if (res?.kind == "never") return pre + "error(\"a never call returned\")"
        val read = if (resultType != null) "${ws[0]}!!.unsafeCast<$resultType>()" else res?.let { fromWire(it, ws) }
        return pre + listOfNotNull(read?.let { "return $it" })
    }

    private val boxImports = HashMap<String, String>()

    /** The flat external of a box helper's op, declared once. */
    private fun boxImport(op: BoxOp): String = "$flatPkg." + boxImports.getOrPut("${op.import.module} ${op.import.name}") {
        val base = if (op.import.module == "tw:box") "box_" else "unbox_"
        flatName(base + op.import.name).also { flatOut.append(flatImport(it, op.import, op.params, op.results, "")).append('\n') }
    }

    /** Pops of `("tw:ret", vt)`, and pushes of `("tw:ret", "push:vt")`, by value type. */
    private val pops = HashMap<String, String>()
    private val pushes = HashMap<String, String>()

    /** Slot `vt` of the last spilled result. */
    private fun pop(vt: String): String = "$flatPkg." + pops.getOrPut(vt) {
        flatName("ret_$vt").also { flatOut.append(flatImport(it, Import("tw:ret", vt), emptyList(), listOf(vt), "")).append('\n') }
    } + "()"

    private fun push(vt: String, e: String): String = "$flatPkg." + pushes.getOrPut(vt) {
        flatName("ret_push_$vt").also { flatOut.append(flatImport(it, Import("tw:ret", "push:$vt"), listOf(vt), emptyList(), "")).append('\n') }
    } + "($e)"

    /**
     * `call` as statements appended to `pre`, and an expression for each of
     * its wire values: slot 0 is the call's result, and slots 1.. are
     * popped right after it, in slot order.
     */
    private fun spilled(call: String, results: List<String>, spill: List<String>, pre: MutableList<String>): List<String> {
        if (results.isEmpty()) {
            pre += call
            return emptyList()
        }
        val r = fresh("r")
        pre += "val $r = $call"
        return listOf(r) + spill.map { vt -> fresh("s").also { pre += "val $it = ${pop(vt)}" } }
    }

    /**
     * Returns wire values `ws` from a Kotlin function whose core results are
     * `results` then `spill`: slots 1.. are pushed in slot order, then slot 0
     * is returned. Every value is computed before the first push.
     */
    private fun returned(ws: List<String>, results: List<String>, spill: List<String>): List<String> {
        if (results.isEmpty()) return emptyList()
        if (ws.size != 1 + spill.size) unsupported("a result of ${ws.size} values for ${1 + spill.size} slots")
        if (spill.isEmpty()) return listOf("return ${ws[0]}")
        val vs = ws.map { fresh("w") }
        return vs.zip(ws).map { (v, e) -> "val $v = $e" } + spill.mapIndexed { i, vt -> push(vt, vs[i + 1]) } + "return ${vs[0]}"
    }

    /** `call`, rethrowing a JS exception as the typed exception of `t`. */
    private fun guarded(call: String, t: ThrowsL?): String {
        if (t == null) return call
        return "try { $call } catch (e: kotlin.js.JsException) { throw ${errors(t.enum).raise(t.read)}(e) }"
    }

    /** The typed exception of a `throws` or rejection enum, and its conversions through the protocol. */
    private inner class Errors(val l: EnumL) {
        val base = sanitize(if (l.origin == "err") "Err" else typeName(l.path))
        val exception = "${base}Exception"
        val text = StringBuilder()
        private val made = HashMap<String, String>()

        init {
            val name = enumKt(l)
            val fallback = l.arms.firstOrNull { it.fallback && it.payload.size == 1 && it.payload[0].kind == "host" && it.payload[0].of == "box" }
            val message = if (l.arms.all { it.payload.size == 1 && it.payload[0].kind == "host" && it.payload[0].of == "box" }) {
                "when (error) {\n" + l.arms.joinToString("") { "    is $name.${q(sanitize(it.name))} -> error.value.toString()\n" } + "}"
            } else "error.toString()"
            text.append("/** `${l.path}`, thrown across the boundary. */\n")
            text.append("class $exception(val error: $name) : kotlin.Exception(${base.replaceFirstChar { it.lowercase() }}Message(error))\n\n")
            text.append("private fun ${base.replaceFirstChar { it.lowercase() }}Message(error: $name): String = $message\n\n")
            text.append("/** `e` as `${l.path}`: its own error, or the fallback arm with its message. */\n")
            val other = if (fallback == null) "throw e" else "$name.${q(sanitize(fallback.name))}((e.message ?: e.toString()).toJsString())"
            text.append("fun to$base(e: kotlin.Throwable): $name = if (e is $exception) e.error else $other\n\n")
        }

        private fun wireParams() = listOf("i32") + l.slots

        private fun decodeFrom(call: String, body: MutableList<String>): String {
            val ws = spilled(call, listOf("i32"), l.slots, body)
            return "$exception(${decoder(l)}(${ws.joinToString()})!!)"
        }

        /** `raiseX(e: JsException)`: the exception's `E` through `("tw:err", E)`. */
        fun raise(read: Import): String = made.getOrPut("raise") {
            val flat = flatName("err_${l.path}")
            flatOut.append(flatImport(flat, read, listOf("externref"), listOf("i32"), "")).append('\n')
            val body = mutableListOf<String>()
            val ex = decodeFrom("$flatPkg.$flat(e.thrownValue)", body)
            text.append(block("fun raise$base(e: kotlin.js.JsException): $exception", body + "return $ex", 0)).append('\n')
            "raise$base"
        }

        /** `setX(e: Throwable)`: `e` as an `E` through `("tw:err", "set:E")`. */
        fun set(set: Import): String = made.getOrPut("set") {
            val flat = flatName("err_set_${l.path}")
            flatOut.append(flatImport(flat, set, wireParams(), emptyList(), "")).append('\n')
            val body = listOf("val w = ${encoder(l)}(to$base(e))", "$flatPkg.$flat(${(listOf("w.arm") + l.slots.indices.map { "w.s$it" }).joinToString()})")
            text.append(block("fun set$base(e: kotlin.Throwable)", body, 0)).append('\n')
            "set$base"
        }

        /** `futureErrorX(f)`: a rejected future's `E` through `error<E>`. */
        fun futureError(op: Import): String = made.getOrPut("error") {
            val flat = flatName("error_${l.path}")
            flatOut.append(flatImport(flat, op, listOf("externref"), listOf("i32"), "")).append('\n')
            val body = mutableListOf<String>()
            val ex = decodeFrom("$flatPkg.$flat(f)", body)
            text.append(block("fun futureError$base(f: JsAny): $exception", body + "return $ex", 0)).append('\n')
            "futureError$base"
        }

        /** `rejectX(resolver, e)`: rejects a language-created future through `reject<E>`. */
        fun reject(op: Import): String = made.getOrPut("reject") {
            val flat = flatName("reject_${l.path}")
            flatOut.append(flatImport(flat, op, listOf("externref") + wireParams(), emptyList(), "")).append('\n')
            val body = listOf("val w = ${encoder(l)}(to$base(e))", "$flatPkg.$flat(${(listOf("resolver", "w.arm") + l.slots.indices.map { "w.s$it" }).joinToString()})")
            text.append(block("fun reject$base(resolver: JsAny?, e: kotlin.Throwable)", body, 0)).append('\n')
            "reject$base"
        }
    }

    private val errorTypes = LinkedHashMap<String, Errors>()

    private fun errors(l: EnumL) = errorTypes.getOrPut(l.path) { Errors(l) }

    private val futureOps = HashMap<String, String>()

    /** A flat import of a future operation, once per name. */
    private fun futureOp(imp: Import, params: List<String>, results: List<String>): String = futureOps.getOrPut(imp.name) {
        flatName("future_" + imp.name.replace("<", "_").replace(">", "")).also {
            flatOut.append(flatImport(it, imp, params, results, "")).append('\n')
        }
    }

    private fun paramNames(m: MemberL) = m.params.map { q(sanitize(it.name)) }

    private class Group {
        val methods = StringBuilder()
        val statics = StringBuilder()
    }

    private val groups = LinkedHashMap<String, Group>()
    private class Ns {
        val members = StringBuilder()
        val children = sortedMapOf<String, Ns>()
    }

    private val namespaces = Ns()

    private val topLevel = StringBuilder()

    private fun nsText(path: List<String>) = path.fold(namespaces) { n, seg -> n.children.getOrPut(seg) { Ns() } }.members

    private fun nsRender(name: String, n: Ns): String {
        val inner = StringBuilder(n.members)
        for ((c, child) in n.children) inner.append(nsRender(c, child).prependIndent("    ")).append("\n\n")
        return "object ${q(sanitize(name))} {\n${inner.toString().trimEnd()}\n}"
    }

    private fun ind(lines: List<String>, n: Int) = lines.joinToString("") { " ".repeat(n) + it + "\n" }

    /** `fun head {` with `lines` as its body, at indent `n`. */
    private fun block(head: String, lines: List<String>, n: Int) = " ".repeat(n) + "$head {\n" + ind(lines, n + 4) + " ".repeat(n) + "}\n"

    private fun property(head: String, get: List<String>, set: List<String>?, n: Int): String {
        val pad = " ".repeat(n)
        val s = StringBuilder("$pad$head\n").append(block("get()", get, n + 4))
        if (set != null) s.append(block("set(value)", set, n + 4))
        return s.toString()
    }

    private fun importMember(m: MemberL) {
        if (m.kind == "set" && m.key in usedSetters) return
        val sig = m.sig ?: unsupported(m.error ?: "no layout")
        tmp = 0
        val fname = flatName(m.key)
        val flat = flatImport(fname, Import(m.module!!, m.key), sig.coreParams, sig.coreResults, kdoc("", m.doc))
        val owner = m.owner?.let { classes[it] }
        val names = paramNames(m)
        val ps = m.params.indices.map { i -> sig.params.first { it.role == "param" && it.index == i }.repr!! }
        val psDecl = names.indices.joinToString { i -> "${names[i]}: ${paramKt(m.params[i], ps[i])}" }
        val args = names.indices.associateWith { names[it] }
        val res = sig.results.firstOrNull()?.repr
        val ctorType = if (m.kind == "new" && owner != null) ownerType(owner) else null
        val resKt = ctorType?.let { ": $it" } ?: res?.let { ": ${kt(it)}" } ?: ""
        val tps = (owner?.tparams.orEmpty() + m.tparams).map { it.name }
        val mtps = tparamDecl(m.tparams.map { it.name })
        val static = owner == null || m.isStatic || m.kind == "new"
        val scope = m.namespace.joinToString(".")
        fun sigKey(n: String, types: List<String> = ps.map { kt(it) }) = "${owner?.path ?: scope}|$static|${m.kind}|$n(${types.joinToString()})"
        val name = q(sanitize(m.name ?: "new")).let { n ->
            val variant = m.variant?.takeIf { m.kind == "method" || m.kind == "new" }?.let { q(sanitize(it)) }
            when {
                declared.add(sigKey(n)) -> n
                variant != null && variant != n && declared.add(sigKey(variant)) -> variant
                else -> unsupported("a duplicate Kotlin declaration of `$n`")
            }
        }
        fun body(receiver: String) = importBody(m, sig, fname, args, receiver, ctorType)
        fun doc(n: Int) = kdoc(" ".repeat(n), m.doc)
        // A payload enum parameter also takes each single-payload arm's value directly.
        fun overloads(head: String, callee: String, targs: List<String>, n: Int): String {
            val types = ps.map { kt(it) }
            val call = callee + if (targs.isEmpty()) "" else "<${targs.joinToString { sanitize(it) }}>"
            val out = StringBuilder()
            ps.forEachIndexed { i, r ->
                if (r.kind != "enum" || r.enum.shape != "payload" || r.enum.origin != "decl") return@forEachIndexed
                for (a in r.enum.arms) {
                    val p = a.payload.singleOrNull()?.takeIf { it.kind != "erased" } ?: continue
                    val t = try { kt(p) } catch (e: Unsupported) { continue }
                    val ts = types.toMutableList().also { it[i] = t }
                    if (t == "JsAny?" || !declared.add(sigKey(name, ts))) continue
                    val decl = names.zip(ts).joinToString { (pn, pt) -> "$pn: $pt" }
                    val args = names.mapIndexed { j, pn -> if (j == i) "${types[i].removeSuffix("?")}.${q(sanitize(a.name))}($pn)" else pn }
                    out.append(" ".repeat(n)).append("fun $head($decl)$resKt = $call(${args.joinToString()})\n")
                }
            }
            return if (out.isEmpty()) "" else "$out\n"
        }
        when {
            owner == null && (m.kind == "get" || m.kind == "set") -> {
                val out = if (m.namespace.isEmpty()) topLevel else nsText(m.namespace)
                val n = if (m.namespace.isEmpty()) 0 else 4
                val clash = m.namespace.isEmpty() && this.names.values.contains(sanitize(m.name!!))
                if (m.kind == "set") {
                    val setName = "set${upperFirst(sanitize(m.name!!))}"
                    out.append(doc(n)).append(block("fun $setName($psDecl)", body(""), n)).append('\n')
                    out.append(overloads(setName, setName, emptyList(), n))
                } else {
                    val getBody = body("")
                    val setBody = if (clash) null else foldedSetter(scope to m.name, res, "")
                    when {
                        clash -> out.append(doc(n)).append(block("fun get${upperFirst(sanitize(m.name))}()$resKt", getBody, n))
                        setBody != null -> out.append(doc(n)).append(property("var $name$resKt", getBody, setBody, n))
                        else -> out.append(doc(n)).append(property("val $name$resKt", getBody, null, n))
                    }
                    out.append('\n')
                }
            }
            owner == null && m.namespace.isEmpty() ->
                topLevel.append(doc(0)).append(block("fun $mtps$name($psDecl)$resKt", body(""), 0)).append('\n')
                    .append(overloads("$mtps$name", name, m.tparams.map { it.name }, 0))
            owner == null ->
                nsText(m.namespace).append(doc(4)).append(block("fun $mtps$name($psDecl)$resKt", body(""), 4)).append('\n')
                    .append(overloads("$mtps$name", name, m.tparams.map { it.name }, 4))
            m.kind == "new" && m.name == null -> {
                val t = typeName(owner.path)
                topLevel.append(doc(0)).append(block("fun ${tparamDecl(tps)}$t($psDecl)$resKt", body(""), 0)).append('\n')
                    .append(overloads("${tparamDecl(tps)}$t", t, tps, 0))
            }
            m.isStatic || m.kind == "new" -> {
                val g = groups.getOrPut(owner.path) { Group() }
                g.statics.append(doc(4)).append(
                    when (m.kind) {
                        "get" -> property("val $name$resKt", body(""), null, 4)
                        "set" -> block("fun set${upperFirst(sanitize(m.name!!))}($psDecl)", body(""), 4) +
                            overloads("set${upperFirst(sanitize(m.name))}", "set${upperFirst(sanitize(m.name))}", emptyList(), 4)
                        "new" -> block("fun ${tparamDecl(tps)}$name($psDecl)$resKt", body(""), 4) + overloads("${tparamDecl(tps)}$name", name, tps, 4)
                        else -> block("fun $mtps$name($psDecl)$resKt", body(""), 4) + overloads("$mtps$name", name, m.tparams.map { it.name }, 4)
                    },
                ).append('\n')
            }
            else -> {
                val g = groups.getOrPut(owner.path) { Group() }
                val recv = ownerType(owner)
                val all = tparamDecl(tps)
                when (m.kind) {
                    "get" -> {
                        val getBody = body("this")
                        val setBody = foldedSetter(owner.path to m.name, res, "this")
                        val head = (if (setBody != null) "var " else "val ") + "$all$recv.$name$resKt"
                        g.methods.append(doc(0)).append(property(head, getBody, setBody, 0)).append('\n')
                    }
                    "set" -> {
                        val setName = "set${upperFirst(sanitize(m.name!!))}"
                        g.methods.append(doc(0)).append(block("fun $all$recv.$setName($psDecl)", body("this"), 0)).append('\n')
                            .append(overloads("$all$recv.$setName", setName, tps, 0))
                    }
                    else -> g.methods.append(doc(0)).append(block("fun $all$recv.$name($psDecl)$resKt", body("this"), 0)).append('\n')
                        .append(overloads("$all$recv.$name", name, tps, 0))
                }
            }
        }
        flatOut.append(flat).append('\n')
    }

    /**
     * The body of the setter that folds into a getter's `var`: the first
     * one in `scope` taking the getter's type `res`. Its member is then
     * passed over.
     */
    private fun foldedSetter(scope: Pair<String, String?>, res: Repr?, receiver: String): List<String>? {
        val setter = pendingSetters[scope]?.takeIf { s ->
            s.sig?.params?.firstOrNull { it.role == "param" }?.repr?.let { kt(it) } == res?.let { kt(it) }
        } ?: return null
        val sig = setter.sig!!
        val fname = flatName(setter.key)
        val flat = flatImport(fname, Import(setter.module!!, setter.key), sig.coreParams, sig.coreResults, kdoc("", setter.doc))
        val body = importBody(setter, sig, fname, mapOf(0 to "value"), receiver)
        usedSetters += setter.key
        flatOut.append(flat).append('\n')
        return body
    }

    private val pendingSetters = HashMap<Pair<String, String?>, MemberL>()
    private val declared = HashSet<String>()
    private val usedSetters = HashSet<String>()

    private fun exportMember(m: MemberL) {
        val sig = m.sig ?: unsupported(m.error ?: "no layout")
        tmp = 0
        val owner = m.owner?.let { classes[it] }
        val wires = mutableListOf<String>()
        val args = mutableListOf<String>()
        var handle: String? = null
        var w = 0
        for (p in sig.params) {
            val ws = p.wire.map { "p${w++}" }
            wires += p.wire
            when (p.role) {
                "this" -> {}
                "handle" -> handle = fromWire(p.repr!!, ws)
                "param" -> args += fromWire(p.repr!!, ws)
                else -> unsupported("an export position `${p.role}`")
            }
        }
        val params = sig.params.flatMap { it.wire }.mapIndexed { i, t -> "p$i: ${wireKt(t)}" }
        val name = sanitize(m.name ?: "new")
        val call = when {
            owner == null -> {
                val fn = (m.namespace.filter { it != "default" } + name).joinToString("_")
                appFn(fn, m, sig)
                "$appPkg.${q(fn)}(${args.joinToString()})"
            }
            m.kind == "new" -> {
                appClassMember(owner, m, sig)
                "${appClass(owner.path)}(${args.joinToString()})"
            }
            m.isStatic -> {
                appClassMember(owner, m, sig)
                "${appClass(owner.path)}.${q(name)}(${args.joinToString()})"
            }
            else -> {
                val folded = m.kind == "set" && foldsExportSetter(owner, m, sig)
                appClassMember(owner, m, sig, folded)
                val self = handle ?: unsupported("an instance export without a handle")
                when {
                    m.kind == "get" -> "$self.${q(name)}"
                    folded -> "$self.${q(name)} = ${args[0]}"
                    m.kind == "set" -> "$self.${q("set" + upperFirst(name))}(${args.joinToString()})"
                    else -> "$self.${q(name)}(${args.joinToString()})"
                }
            }
        }
        val res = sig.results.firstOrNull()
        val resKt = sig.coreResults.firstOrNull()?.let { ": ${wireKt(it)}" } ?: ""
        val future = res?.repr?.takeIf { it.kind == "host" && it.of == "future" }
        val body = when {
            res == null -> listOf(call)
            future != null -> {
                if (future.nullable) unsupported("a nullable future export result")
                futureExport(future, call)
            }
            else -> {
                val c = toWire(res.repr!!, "r")
                listOf("val r = $call") + c.pre + returned(c.exprs, sig.coreResults, sig.coreSpill)
            }
        }.let { b ->
            val t = sig.throws ?: return@let b
            val set = errors(t.enum).set(t.set)
            listOf("try {") + b.map { "    $it" } + "} catch (e: kotlin.Throwable) {" +
                ("    $set(e)") + listOfNotNull(sig.coreResults.firstOrNull()?.let { "    return ${zero(it)}" }) + "}"
        }
        val fname = flatName("export_${m.key}")
        exportOut.append(kdoc("", m.doc))
            .append("${exportAnn(m.key, wires + sig.coreResults)}\nfun $fname(${params.joinToString()})$resKt {\n")
            .append(body.joinToString("") { "    $it\n" }).append("}\n\n")
        if (owner != null) m.construct?.let { construct(owner, m, it) }
    }

    /**
     * `new<Class>(...)`: constructs an exported class with an imported base
     * through JS (`tw:new`), which runs its JS constructor and re-enters the
     * constructor export, and recovers the instance with `tw:unbox`. A
     * base-less class is constructed natively and wrapped on crossing.
     */
    private fun construct(owner: DeclL, m: MemberL, sig: Sig) {
        tmp = 0
        val fname = flatName("new_${m.key}")
        flatOut.append(flatImport(fname, Import("tw:new", m.key), sig.coreParams, sig.coreResults, "")).append('\n')
        val unbox = boxImport(owner.unbox ?: BoxOp(Import("tw:unbox", owner.path), listOf("externref"), listOf("externref"), emptyList()))
        val names = paramNames(m)
        val args = names.indices.associateWith { names[it] }
        val app = appClass(owner.path)
        val b = importBody(m, sig, fname, args, "", "JsAny")
        val body = b.dropLast(1) + "return $unbox(${b.last().removePrefix("return ")})!!.unsafeCast<JsReference<$app>>().get()"
        val psDecl = m.params.indices.joinToString { i ->
            "${names[i]}: ${paramKt(m.params[i], sig.params.first { it.role == "param" && it.index == i }.repr!!)}"
        }
        val variant = m.name?.let { upperFirst(sanitize(it)) } ?: ""
        exportOut.append("/** A new `${typeName(owner.path)}` constructed through JS. */\n")
            .append(block("fun new${typeName(owner.path)}$variant($psDecl): $app", body, 0)).append("\n")
    }

    private fun appParams(m: MemberL, sig: Sig) = m.params.indices.joinToString { i ->
        "${q(sanitize(m.params[i].name))}: ${paramKt(m.params[i], sig.params.first { it.role == "param" && it.index == i }.repr!!)}"
    }

    /** A parameter's Kotlin type, by its alias when Kotlin declares one. */
    private fun paramKt(p: ParamL, r: Repr) = p.alias?.takeIf { it in typeAliases }?.let { typeName(it) + opt(r) } ?: kt(r)

    /** The aliases Kotlin declares as a `typealias`: those without type parameters that don't recur. */
    private val typeAliases: Map<String, String> by lazy {
        val out = LinkedHashMap<String, String>()
        for (d in low.decls) {
            if (d.kind != "alias" || d.recursive || d.repr == null) continue
            runCatching { kt(d.repr) }.onSuccess { out[d.path] = it }
        }
        out
    }

    private fun aliasText(): String {
        val out = StringBuilder()
        for ((path, t) in typeAliases) out.append("typealias ${typeName(path)} = $t\n\n")
        var i = 0
        while (i < funInterfaces.size) {
            val rec = funInterfaces.values.elementAt(i++)
            val f = rec.toLang
            val ps = f.fnParams.mapIndexed { j, p -> "a$j: ${kt(p)}" }
            out.append("/** `${rec.ty}`, which names itself. */\nfun interface ${sanitize(rec.name)} {\n")
                .append("    operator fun invoke(${ps.joinToString()}): ${kt(f.fnResult)}\n}\n\n")
        }
        return out.toString()
    }

    private fun appResult(sig: Sig) = sig.results.firstOrNull()?.repr?.let { r ->
        if (r.kind == "host" && r.of == "future") ": ${kt(r.value)}" else ": ${kt(r)}"
    } ?: ""

    /** An export returning a future is a `suspend` function, run as a task. */
    private fun appSuspend(sig: Sig) =
        if (sig.results.firstOrNull()?.repr?.let { it.kind == "host" && it.of == "future" } == true) "suspend " else ""

    private fun appBody(sig: Sig, what: String) =
        appResult(sig).let { if (it.isEmpty()) " { TODO(${lit(what)}) }" else "$it = TODO(${lit(what)})" }

    private fun appFn(fn: String, m: MemberL, sig: Sig) {
        appOut.append("${appSuspend(sig)}fun ${q(fn)}(${appParams(m, sig)})${appBody(sig, fn)}\n\n")
    }

    /**
     * An export setter folds into its getter's `var` when it takes the
     * getter's type; the first such setter wins, and the rest are `setX` funs.
     */
    private fun foldsExportSetter(owner: DeclL, m: MemberL, sig: Sig): Boolean {
        val value = sig.params.filter { it.role == "param" }.singleOrNull()?.repr ?: return false
        val getter = low.members.firstOrNull {
            !it.import && it.owner == m.owner && it.kind == "get" && !it.isStatic && it.name == m.name
        }?.sig?.results?.singleOrNull()?.repr ?: return false
        return kt(getter) == kt(value) && foldedExportSetters.add(owner.path to m.name)
    }

    private val vecs = HashSet<String>()

    /** Arities of the generated `TupleN` classes: Kotlin has only `Pair` and `Triple`. */
    private val tuples = sortedSetOf<Int>()

    private val ordinals = listOf("first", "second", "third", "fourth", "fifth", "sixth", "seventh", "eighth")

    private fun tupleField(i: Int) = ordinals.getOrElse(i) { "v$it" }

    private fun tupleClass(n: Int) = when (n) {
        2 -> "Pair"
        3 -> "Triple"
        else -> "Tuple$n".also { tuples += n }
    }

    private fun vecKt(elem: String): String {
        vecs += elem
        return "Vec${upperFirst(elem)}"
    }

    private val foldedExportSetters = HashSet<Pair<String, String?>>()

    private fun appClassMember(owner: DeclL, m: MemberL, sig: Sig, folded: Boolean = false) {
        val list = appClasses.getOrPut(owner.path) { mutableListOf() }
        val name = q(sanitize(m.name ?: "new"))
        list += when {
            m.kind == "new" -> "new(${appParams(m, sig)})"
            m.isStatic -> "static ${appSuspend(sig)}fun $name(${appParams(m, sig)})${appBody(sig, m.name ?: "")}"
            m.kind == "get" -> "val $name${appResult(sig)} get() = TODO(${lit(m.name!!)})"
            m.kind == "set" && folded -> "set $name"
            m.kind == "set" -> "fun ${q("set" + upperFirst(sanitize(m.name!!)))}(${appParams(m, sig)}) { TODO(${lit(m.name)}) }"
            else -> "${appSuspend(sig)}fun $name(${appParams(m, sig)})${appBody(sig, m.name!!)}"
        }
    }

    private fun trampoline(ty: String, r: Repr): String {
        tmp = 0
        eraseToAny = true
        try {
            return trampolineText(ty, r)
        } finally {
            eraseToAny = false
        }
    }

    private fun trampolineText(ty: String, r: Repr): String {
        val op = r.op ?: unsupported("no trampoline signature for `$ty`")
        val fnType = recOf(r)?.let { funInterface(it) } ?: "(${r.fnParams.joinToString { kt(it) }}) -> ${kt(r.fnResult)}"
        var w = 1
        val args = r.fnParams.map { p -> fromWire(p, p.wire.map { "p${w++}" }) }
        val params = op.coreParams.mapIndexed { i, t -> "p$i: ${wireKt(t)}" }
        val call = "f(${args.joinToString()})"
        val res = r.fnResult
        val body = mutableListOf("val f = p0!!.unsafeCast<JsReference<$fnType>>().get()")
        if (res.kind == "unit" || res.kind == "never") body += call else {
            val c = toWire(res, "r")
            body += "val r = $call"
            body += c.pre
            body += returned(c.exprs, op.coreResults, op.coreSpill)
        }
        val resKt = op.coreResults.firstOrNull()?.let { ": ${wireKt(it)}" } ?: ""
        return "${exportAnn(r.trampoline, op.coreParams + op.coreResults)}\nfun ${flatName("trampoline_$ty")}(${params.joinToString()})$resKt {\n" +
            body.joinToString("") { "    $it\n" } + "}\n\n"
    }

    private fun encoder(l: EnumL) = codec("encode", l)
    private fun decoder(l: EnumL) = codec("decode", l)

    private fun codec(op: String, l: EnumL) = libNames[l.path]?.let {
        it.substringBeforeLast('.') + ".$op" + sanitize(it.substringAfterLast('.'))
    } ?: "$op${sanitize(enumIdent(l))}"

    private fun enumIdent(l: EnumL) = when (l.origin) {
        "err" -> "Err"
        "nested" -> "Nested_" + sanitize(l.path)
        else -> typeName(l.path)
    }

    private fun enumText(l: EnumL): String {
        val name = enumKt(l)
        val out = StringBuilder()
        val slots = l.slots
        val wireClass = "${sanitize(enumIdent(l))}Wire"
        fun wireDecls() = (listOf("val arm: Int") + slots.mapIndexed { i, t -> "val s$i: ${wireKt(t)}" }).joinToString()
        fun zeros() = slots.map { zero(it) }
        fun armWire(a: ArmL, values: List<String>): String {
            val convs = a.payload.zip(values).map { (p, v) -> toWire(p, v) }
            val exprs = convs.flatMap { it.exprs }
            val s = zeros().toMutableList()
            a.slots.forEachIndexed { j, slot -> s[slot] = exprs[j] }
            val pre = convs.flatMap { it.pre }
            val ctor = "$wireClass(${(listOf(a.index.toString()) + s).joinToString()})"
            return if (pre.isEmpty()) ctor else "{ ${pre.joinToString("; ")}; $ctor }"
        }
        fun armRead(a: ArmL): List<String> {
            var i = 0
            return a.payload.map { p ->
                val ws = (0 until p.wire.size).map { "s${a.slots[i++]}" }
                fromWire(p, ws)
            }
        }
        when {
            l.origin == "nested" -> {
                val v = l.arms.first { it.name == "Value" }
                val u = l.arms.first { it.name == "Undefined" }
                out.append("class $wireClass(${wireDecls()})\n\n")
                out.append("fun ${encoder(l)}(v: $name): $wireClass =\n    if (v == null) $wireClass(${(listOf(u.index.toString()) + zeros()).joinToString()}) else ${armWire(v, listOf("v"))}\n\n")
                out.append("fun ${decoder(l)}(${wireDecls().replace("val ", "")}): $name = when (arm) {\n    ${u.index} -> null\n    else -> ${armRead(v)[0]}\n}\n\n")
            }
            l.shape == "index" -> {
                out.append("enum class $name(val index: Int) {\n")
                out.append(l.arms.joinToString(",\n") { "    ${q(sanitize(it.name))}(${it.index})" }).append(";\n\n")
                out.append("    companion object {\n        fun fromIndex(index: Int): $name = entries.first { it.index == index }\n    }\n}\n\n")
            }
            l.shape == "carrier" -> {
                val c = l.carrierScalar!!
                out.append("value class $name(val value: ${scalarKt(c)}) {\n    companion object {\n")
                for (a in l.arms) out.append("        val ${q(sanitize(a.name))} = $name(${sLit(c, a.index)})\n")
                out.append("    }\n}\n\n")
            }
            else -> {
                out.append("sealed interface $name {\n")
                for (a in l.arms) {
                    val an = q(sanitize(a.name))
                    if (a.payload.isEmpty()) out.append("    data object $an : $name\n")
                    else {
                        qual = "$pkg."
                        val fields = try {
                            a.payload.mapIndexed { i, p -> "val ${if (a.payload.size == 1) "value" else "v$i"}: ${kt(p)}" }
                        } finally {
                            qual = ""
                        }
                        out.append("    data class $an(${fields.joinToString()}) : $name\n")
                    }
                }
                out.append("}\n\n")
                out.append("class $wireClass(${wireDecls()})\n\n")
                out.append("fun ${encoder(l)}(v: $name?): $wireClass = when (v) {\n")
                out.append("    null -> $wireClass(${(listOf("0") + zeros()).joinToString()})\n")
                for (a in l.arms) {
                    val an = "$name.${q(sanitize(a.name))}"
                    if (a.payload.isEmpty()) out.append("    $an -> ${armWire(a, emptyList())}\n")
                    else {
                        val vs = a.payload.indices.map { if (a.payload.size == 1) "v.value" else "v.v$it" }
                        val w = try { armWire(a, vs) } catch (e: Unsupported) { "throw UnsupportedOperationException(${lit(e.message ?: "")})" }
                        out.append("    is $an -> $w\n")
                    }
                }
                out.append("}\n\n")
                out.append("fun ${decoder(l)}(${wireDecls().replace("val ", "")}): $name? = when (arm) {\n    0 -> null\n")
                for (a in l.arms) {
                    val an = "$name.${q(sanitize(a.name))}"
                    val read = if (a.payload.isEmpty()) an else "$an(${armRead(a).joinToString()})"
                    out.append("    ${a.index} -> $read\n")
                }
                out.append("    else -> throw IllegalStateException(\"${name}: arm \$arm\")\n}\n\n")
            }
        }
        return out.toString()
    }

    /**
     * `C.toJs()` and `JsAny.toC()`: an exported class's instance as its box,
     * the JS wrapper an erased position holds, and back (§13).
     */
    private fun classBox(d: DeclL): String {
        val (box, unbox) = handleOps(d.box, d.unbox) ?: return ""
        if (d.tparams.isNotEmpty()) return ""
        val app = appClass(d.path)
        val name = typeName(d.path)
        return "/** `$name` as its box: the JS wrapper an erased position holds. */\n" +
            "fun $app.toJs(): JsAny = $box(this.toJsReference())!!\n\n" +
            "/** The `$name` a box holds; a `JsException` unless it's a live `$name` wrapper. */\n" +
            "fun JsAny.to$name(): $app = $unbox(this)!!.unsafeCast<JsReference<$app>>().get()\n\n"
    }

    /** `F.toJs()` and `JsAny.toFn…()` for a function type the language provides. */
    private fun callbackBox(r: Repr): String {
        val (box, unbox) = handleOps(r.box, r.unbox) ?: return ""
        tmp = 0
        eraseToAny = true
        try {
            val ps = r.fnParams.map { kt(it) }
            val res = kt(r.fnResult)
            val fnType = recOf(r)?.let { funInterface(it) } ?: "(${ps.joinToString()}) -> $res"
            val name = "Fn" + ps.joinToString("") { ktIdent(it) } + "To" + ktIdent(res)
            if (!boxHelpers.add(name)) return ""
            return "/** A `${r.ty.removeSuffix("?")}` as its box: a JS function calling it. */\n" +
                "fun ($fnType).toJs(): JsAny = $box(this.toJsReference())!!\n\n" +
                "/** The function a box holds; a `JsException` unless it's one this module provided. */\n" +
                "fun JsAny.to$name(): $fnType = $unbox(this)!!.unsafeCast<JsReference<$fnType>>().get()\n\n"
        } finally {
            eraseToAny = false
        }
    }

    /** `E.toJs()` and `JsAny.toE()`: an enum as its box, its arm's JS value, and back. */
    private fun enumBox(d: DeclL): String {
        val box = d.box ?: return ""
        val unbox = d.unbox ?: return ""
        val l = d.enum ?: return ""
        val r = Repr(l.path, box.params, "enum", false, mapOf("enum" to d.raw["layout"]))
        tmp = 0
        val k = enumKt(l)
        val name = typeName(l.path)
        val c = toWire(r, "this")
        val pre = mutableListOf<String>()
        val ws = spilled("${boxImport(unbox)}(this)", unbox.results, unbox.spill, pre)
        val v = fromWire(r, ws)
        return "/** `$name` as its box: its arm's JS value. */\n" +
            block("fun $k.toJs(): JsAny", c.pre + "return ${boxImport(box)}(${c.exprs.joinToString()})!!", 0) + "\n" +
            "/** The `$name` a box holds; a `JsException` unless it's one of its arms. */\n" +
            block("fun JsAny.to$name(): $k", pre + "return $v", 0) + "\n"
    }

    /** A handle's box ops, which take and give one `externref` under host handles. */
    private fun handleOps(box: BoxOp?, unbox: BoxOp?): Pair<String, String>? {
        if (box == null || unbox == null) return null
        if (box.params != listOf("externref") || unbox.results != listOf("externref")) unsupported("box helpers for non-host handles")
        return boxImport(box) to boxImport(unbox)
    }

    private fun ktIdent(t: String) = t.replace("?", "Opt").filter { it.isLetterOrDigit() }

    private val boxHelpers = HashSet<String>()

    private fun guard(key: String, f: () -> Unit) {
        val outer = at
        at = key
        try {
            f()
        } catch (e: Unsupported) {
            skipped += "$key: ${e.message}"
        } finally {
            at = outer
        }
    }

    class Output(
        val files: Map<String, String>,
        val links: List<Pair<String, Import>>,
        val skipped: List<String>,
        val warnings: List<String>,
        val app: String?,
    )

    fun run(): Output {
        for (m in low.members) {
            if (!m.import || m.kind != "set" || m.isStatic) continue
            pendingSetters.putIfAbsent((m.owner ?: m.namespace.joinToString(".")) to m.name, m)
        }
        for (m in low.members) {
            for (w in m.warnings) warnings += "${m.key}: $w"
            guard(m.key) { if (m.import) importMember(m) else exportMember(m) }
        }
        val tramp = StringBuilder()
        var i = 0
        while (i < trampolines.size) {
            val (ty, r) = trampolines.entries.elementAt(i++)
            guard(r.trampoline) { tramp.append(trampoline(ty, r)) }
        }
        for ((ty, r) in trampolines.entries.toList()) guard("box ${ty}") { tramp.append(callbackBox(r)) }
        for (d in low.decls) {
            if (d.kind == "class" && d.export) guard("box ${d.path}") { exportOut.append(classBox(d)) }
        }
        val enumOut = StringBuilder()
        for (d in low.decls) {
            if (d.kind == "enum") guard("box ${d.path}") {
                eraseToAny = true
                try {
                    enumOut.append(enumBox(d))
                } finally {
                    eraseToAny = false
                }
            }
        }
        var j = 0
        while (j < enums.size) {
            val l = enums.values.elementAt(j++)
            guard("enum ${l.path}") {
                eraseToAny = true
                try {
                    enumOut.append(enumText(l))
                } finally {
                    eraseToAny = false
                }
            }
        }
        val pdir = pkg.replace('.', '/')
        val files = LinkedHashMap<String, String>()
        val header = "// Generated by ts-gen-kotlin from the lowered IR. Do not edit.\n"
        files["$pdir/flat/Flat.kt"] = header +
            "@file:OptIn(kotlin.wasm.ExperimentalWasmInterop::class, kotlin.js.ExperimentalJsExport::class)\n\npackage $flatPkg\n\n" +
            flatOut
        val errorOut = errorTypes.values.joinToString("") { it.text.toString() }
        val aliases = aliasText()
        files["$pdir/Bindings.kt"] = header + "@file:OptIn(kotlin.js.ExperimentalWasmJsInterop::class)\n\npackage $pkg\n\n" + aliases + typedText() + enumOut + errorOut
        if (vecs.isNotEmpty()) {
            files["$pdir/Vecs.kt"] = header + "package $pkg\n" +
                vecs.sorted().joinToString("") { "\n/** A `$it[]`: the JS typed array of its element. */\nexternal interface ${vecKt(it)} : JsAny\n" }
        }
        if (tuples.isNotEmpty()) {
            files["$pdir/Tuples.kt"] = header + "package $pkg\n" + tuples.joinToString("") { n ->
                val ps = (0 until n).map { "T$it" }
                val fields = ps.mapIndexed { i, t -> "val ${tupleField(i)}: $t" }
                "\n/** A ${n}-tuple. */\ndata class Tuple$n<${ps.joinToString()}>(${fields.joinToString()})\n"
            }
        }
        if (exportOut.isNotEmpty() || tramp.isNotEmpty()) {
            files["$pdir/Exports.kt"] = header +
                "@file:OptIn(kotlin.wasm.ExperimentalWasmInterop::class, kotlin.js.ExperimentalJsExport::class)\n\npackage $pkg\n\n" +
                "import kotlin.wasm.WasmExport\n\n" + exportOut + tramp
        }
        return Output(files, links, skipped, warnings.toList(), appText())
    }

    private fun typedText(): String {
        val out = StringBuilder()
        for (d in classes.values) {
            if (d.kind == "class" && d.export || d.path in libNames) continue
            val parents = d.extends.mapNotNull { e ->
                val base = e.substringBefore('<')
                val pd = classes[base]?.takeIf { !(it.kind == "class" && it.export) } ?: return@mapNotNull null
                typeName(pd.path) + if (pd.tparams.isEmpty()) "" else "<${pd.tparams.joinToString { erased(it) }}>"
            }.ifEmpty { listOf("JsAny") }
            out.append(kdoc("", assocDoc(d)))
            out.append("external interface ${typeName(d.path)}${if (d.tparams.isEmpty()) "" else "<${d.tparams.joinToString { "${sanitize(it.name)} : JsAny?" }}>"} : ${parents.joinToString()}\n\n")
            groups[d.path]?.let { g ->
                out.append(g.methods)
                if (g.statics.isNotEmpty()) out.append("object ${typeName(d.path)}Static {\n").append(g.statics.toString().trimEnd()).append("\n}\n\n")
            }
        }
        out.append(topLevel)
        for ((name, n) in namespaces.children) out.append(nsRender(name, n)).append("\n\n")
        return out.toString()
    }

    /** `d`'s doc, noting the row each associated type reads as: an erased parameter reads the first. */
    private fun assocDoc(d: DeclL): String? {
        if (d.assoc.isEmpty()) return d.doc
        val notes = d.assoc.joinToString("\n") { a -> "`${a.name}` reads as `${a.rows.first().second}`: `${a.param}` is erased to `${a.rows.first().first}`, its first row." }
        return d.doc?.let { "$it\n\n$notes" } ?: notes
    }

    private fun appText(): String? {
        if (appOut.isEmpty() && appClasses.isEmpty()) return null
        val out = StringBuilder("package $appPkg\n\nimport $pkg.*\n\n").append(appOut)
        for ((path, members) in appClasses) {
            val ctor = members.firstOrNull { it.startsWith("new(") }?.removePrefix("new")?.removeSuffix("") ?: "()"
            val statics = members.filter { it.startsWith("static ") }.map { it.removePrefix("static ") }
            val inst = members.filter { !it.startsWith("new(") && !it.startsWith("static ") && !it.startsWith("set ") }
            val setters = members.filter { it.startsWith("set ") }.map { it.removePrefix("set ") }.toSet()
            out.append("class ${typeName(path)}$ctor {\n")
            for (x in inst) {
                val v = if (x.startsWith("val ") && x.removePrefix("val ").substringBefore(':').trim() in setters) "var" + x.removePrefix("val").replace(" get() = ", " = ") else x
                out.append("    $v\n")
            }
            if (statics.isNotEmpty()) out.append("    companion object {\n").append(statics.joinToString("") { "        $it\n" }).append("    }\n")
            out.append("}\n\n")
        }
        return out.toString()
    }
}
