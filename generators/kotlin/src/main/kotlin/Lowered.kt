package tsgen.kotlin

/**
 * The lowered IR (`ts-gen-abi lower`, format `ts-gen-abi/lowered` v1):
 * declarations, and members with their keys and layouts.
 */
class Lowered(
    val profile: Map<String, String>,
    /** The IR's own `lib` name, when it is a lib. */
    val lib: String?,
    /** The libs its `with` names, in resolution order, with their declarations. */
    val libs: List<LibL>,
    val decls: List<DeclL>,
    val members: List<MemberL>,
    /** A recursive alias's function types, by canonical spelling. */
    val recs: Map<String, RecL>,
) {
    companion object {
        const val FORMAT = "ts-gen-abi/lowered"
        const val VERSION = 1L

        fun read(text: String): Lowered {
            val root = JsonReader.read(text).obj("the document")
            if (root["format"] != FORMAT) throw JsonError("not a $FORMAT document")
            if (root["version"] != VERSION) throw JsonError("unsupported version ${root["version"]}")
            return Lowered(
                root.obj("profile").mapValues { it.value as String },
                root.strOrNull("lib"),
                (root["libs"] as List<*>?).orEmpty().map {
                    val l = it.obj("lib")
                    LibL(l.str("name"), l.list("decls").map { d -> DeclL.from(d.obj("decl")) })
                },
                root.list("decls").map { DeclL.from(it.obj("decl")) },
                root.list("members").map { MemberL.from(it.obj("member")) },
                (root["recs"] as List<*>?).orEmpty().map { RecL.from(it.obj("rec")) }.associateBy { it.ty },
            )
        }
    }
}

@Suppress("UNCHECKED_CAST")
fun Any?.obj(what: String): Map<String, Any?> =
    this as? Map<String, Any?> ?: throw JsonError("expected an object for $what")

fun Map<String, Any?>.obj(k: String): Map<String, Any?> = this[k].obj(k)
fun Map<String, Any?>.list(k: String): List<Any?> = this[k] as? List<Any?> ?: throw JsonError("expected a list for $k")
fun Map<String, Any?>.str(k: String): String = this[k] as? String ?: throw JsonError("expected a string for $k")
fun Map<String, Any?>.strOrNull(k: String): String? = this[k] as String?
fun Map<String, Any?>.bool(k: String): Boolean = this[k] as? Boolean ?: false
fun Map<String, Any?>.long(k: String): Long = this[k] as? Long ?: throw JsonError("expected a number for $k")
fun Map<String, Any?>.strs(k: String): List<String> = list(k).map { it as String }

/** A type parameter; [default] is its default's layout, the type it erases to. */
class TParam(val name: String, val default: Repr?) {
    companion object {
        fun from(m: Map<String, Any?>) = TParam(
            m.str("name"),
            (m["default"] as Map<*, *>?)?.let { Repr.from(it.obj("default")) },
        )
    }
}

class LibL(val name: String, val decls: List<DeclL>)

class DeclL(
    /** `namespace`, `module`, `class`, `interface` or `enum`. */
    val kind: String,
    /** The canonical location (`"spec".Path` or the dotted path). */
    val path: String,
    val module: String?,
    val name: String,
    val export: Boolean,
    val tparams: List<TParam>,
    val assoc: List<AssocL>,
    val extends: List<String>,
    val doc: String?,
    val enum: EnumL?,
    /** An alias's layout, when it has no type parameters. */
    val repr: Repr?,
    val recursive: Boolean,
    /** An exported class's or an enum's `tw:box` and `tw:unbox`. */
    val box: BoxOp?,
    val unbox: BoxOp?,
    /** The decl's own JSON, for an enum's layout as a [Repr]'s. */
    val raw: Map<String, Any?>,
) {
    companion object {
        fun from(m: Map<String, Any?>) = DeclL(
            m.str("kind"),
            m.strOrNull("path") ?: m.strOrNull("spec") ?: "",
            m.strOrNull("module"),
            m.strOrNull("name") ?: "",
            m.bool("export"),
            (m["tparams"] as List<*>?).orEmpty().map { TParam.from(it.obj("tparam")) },
            (m["assoc"] as List<*>?).orEmpty().map { AssocL.from(it.obj("assoc")) },
            (m["extends"] as List<*>?).orEmpty().map { it as String },
            m.strOrNull("doc"),
            (m["layout"] as Map<*, *>?)?.let { EnumL.from(it.obj("layout")) },
            (m["repr"] as Map<*, *>?)?.let { Repr.from(it.obj("repr")) },
            m["recursive"] == true,
            BoxOp.of(m, "box"),
            BoxOp.of(m, "unbox"),
            m,
        )
    }
}

/** An associated type: a row of `type` per `arg` of `param`'s bound, in the bound's order. */
class AssocL(val name: String, val param: String, val rows: List<Pair<String, String>>) {
    companion object {
        fun from(m: Map<String, Any?>) = AssocL(
            m.str("name"),
            m.str("param"),
            m.list("rows").map { it.obj("row").let { r -> r.str("arg") to r.str("type") } },
        )
    }
}

/** A box helper's `tw:box` or `tw:unbox` import, and its core type. */
class BoxOp(val import: Import, val params: List<String>, val results: List<String>, val spill: List<String>) {
    companion object {
        fun of(m: Map<String, Any?>, k: String) = (m[k] as Map<*, *>?)?.obj(k)?.let {
            BoxOp(Import.from(it.obj("import")), it.strs("params"), it.strs("results"), it.strs("spill"))
        }
    }
}

class Import(val module: String, val name: String) {
    companion object {
        fun from(m: Map<String, Any?>) = Import(m.str("module"), m.str("name"))
    }
}

/** A param, with the alias its type names, if any. */
class ParamL(val name: String, val once: Boolean, val alias: String?)

/** A function type a recursive alias reaches again: its layouts each way. */
class RecL(val ty: String, val name: String, val toJs: Repr, val toLang: Repr) {
    companion object {
        fun from(m: Map<String, Any?>) = RecL(
            m.str("ty"),
            m.str("name"),
            Repr.from(m.obj("toJs")),
            Repr.from(m.obj("toLang")),
        )
    }
}

class MemberL(
    val key: String,
    /** `import` or `export`. */
    val role: String,
    val module: String?,
    /** The owning class or interface's canonical location. */
    val owner: String?,
    val namespace: List<String>,
    /** `method`, `get`, `set` or `new`. */
    val kind: String,
    val name: String?,
    /** The name among same-named siblings, for when Kotlin conflates their signatures. */
    val variant: String?,
    val isStatic: Boolean,
    val tparams: List<TParam>,
    val params: List<ParamL>,
    val doc: String?,
    val sig: Sig?,
    val error: String?,
    /** The constructor of an exported class with an imported base, read as its `tw:new` import. */
    val construct: Sig?,
    /** Layout approximations, such as a box `T??` read as `T?`. */
    val warnings: List<String>,
) {
    val import get() = role == "import"

    companion object {
        fun from(m: Map<String, Any?>) = MemberL(
            m.str("key"),
            m.str("role"),
            m.strOrNull("module"),
            m.strOrNull("owner"),
            m.strs("namespace"),
            m.str("kind"),
            m.strOrNull("name"),
            m.strOrNull("variant"),
            m.bool("static"),
            m.list("tparams").map { TParam.from(it.obj("tparam")) },
            m.list("params").map { it.obj("param").let { p -> ParamL(p.str("name"), p.bool("once"), p.strOrNull("alias")) } },
            m.strOrNull("doc"),
            (m["sig"] as Map<*, *>?)?.let { Sig.from(it.obj("sig")) },
            m.strOrNull("error"),
            (m["construct"] as Map<*, *>?)?.let { Sig.from(it.obj("construct")) },
            if (m["warnings"] == null) emptyList() else m.strs("warnings"),
        )
    }
}

class Sig(
    val params: List<PosL>,
    val results: List<PosL>,
    val throws: ThrowsL?,
    val coreParams: List<String>,
    /** Slot 0 at most; [coreSpill] holds the rest. */
    val coreResults: List<String>,
    /** The value types of slots 1.., popped or pushed through `tw:ret`. */
    val coreSpill: List<String>,
) {
    companion object {
        fun from(m: Map<String, Any?>): Sig {
            val core = m.obj("core")
            return Sig(
                m.list("params").map { PosL.from(it.obj("position")) },
                m.list("results").map { PosL.from(it.obj("position")) },
                (m["throws"] as Map<*, *>?)?.let { ThrowsL.from(it.obj("throws")) },
                core.strs("params"),
                core.strs("results"),
                core.strs("spill"),
            )
        }
    }
}

/** A `throws` under `throws = js`: its error's arm and slots are read and set through [read] and [set]. */
class ThrowsL(val read: Import, val set: Import, val enum: EnumL) {
    companion object {
        fun from(m: Map<String, Any?>) = ThrowsL(Import.from(m.obj("read")), Import.from(m.obj("set")), EnumL.from(m.obj("enum")))
    }
}

class PosL(
    /** `this`, `handle`, `env`, `callee`, `future`, `index`, `param` or `result`. */
    val role: String,
    val index: Int?,
    /** Absent for a function value's own `env`/`callee` position. */
    val repr: Repr?,
    val wire: List<String>,
) {
    companion object {
        fun from(m: Map<String, Any?>): PosL {
            val repr = (m["repr"] as Map<*, *>?)?.let { Repr.from(it.obj("repr")) }
            return PosL(
                m.str("role"),
                (m["index"] as Long?)?.toInt(),
                repr,
                repr?.wire ?: m.strs("wire"),
            )
        }
    }
}

/** A type's layout (`ts_gen_abi::layout::Repr`). */
class Repr(
    /** The canonical spelling. */
    val ty: String,
    val wire: List<String>,
    /**
     * `unit`, `scalar`, `str`, `vec`, `array`, `tuple`, `enum`, `host`,
     * `handle`, `fn`, `erased` or `view`.
     */
    val kind: String,
    val nullable: Boolean,
    val m: Map<String, Any?>,
) {
    val scalar get() = m.str("scalar")
    /** A nullable scalar's absent value; `null` for the present flag. */
    val niche get() = m["niche"] as Long?
    val carrier get() = m.str("carrier")
    val elem get() = Repr.from(m.obj("elem"))
    val vecElem get() = m.str("elem")
    val elems get() = m.list("elems").map { Repr.from(it.obj("elem")) }
    val enum get() = EnumL.from(m.obj("enum"))
    /** For `host`: `class`, `interface`, `any`, `box` or `future`. */
    val of get() = m.str("of")
    val path get() = m.str("path")
    val inner get() = Repr.from(m.obj("inner"))
    val value get() = Repr.from(m.obj("value"))
    val handleClass get() = m.str("class")
    val fnParams get() = m.list("params").map { Repr.from(it.obj("param")) }
    val fnResult get() = Repr.from(m.obj("result"))
    /** The handle carrier when the language provides the function. */
    val provided get() = m.strOrNull("provided")
    val trampoline get() = m.str("trampoline")
    /** A function type the language provides: its `tw:box` and `tw:unbox`. */
    val box get() = BoxOp.of(m, "box")
    val unbox get() = BoxOp.of(m, "unbox")
    val call get() = Import.from(m.obj("call"))
    /** A future's `take<T>` import. */
    val take get() = Import.from(m.obj("take"))
    /** A future's `error<E>`, `new`, `resolve<T>` and `reject<E>` imports. */
    val errorOp get() = Import.from(m.obj("errorOp"))
    val futureNew get() = Import.from(m.obj("new"))
    val resolve get() = Import.from(m.obj("resolve"))
    val reject get() = Import.from(m.obj("reject"))
    /** A future's rejection enum. */
    val rejection get() = EnumL.from(m.obj("rejection"))
    val op get() = (m["op"] as Map<*, *>?)?.obj("op")?.takeIf { "error" !in it }?.let { Sig.from(it) }

    companion object {
        fun from(m: Map<String, Any?>) = Repr(
            m.str("ty"),
            (m["wire"] as List<*>?)?.map { it as String } ?: emptyList(),
            m.str("kind"),
            m.bool("nullable"),
            m,
        )
    }
}

class ArmL(
    val name: String,
    val index: Long,
    val fallback: Boolean,
    val payload: List<Repr>,
    val slots: List<Int>,
)

/** An enum's layout: `index`, `carrier` or `payload` shape. */
class EnumL(
    val path: String,
    val origin: String,
    val shape: String,
    val carrierScalar: String?,
    val slots: List<String>,
    val arms: List<ArmL>,
) {
    companion object {
        fun from(m: Map<String, Any?>): EnumL {
            val shape = m.obj("shape")
            return EnumL(
                m.str("path"),
                m.str("origin"),
                shape.str("kind"),
                shape.strOrNull("scalar"),
                (shape["slots"] as List<*>?)?.map { it as String } ?: emptyList(),
                m.list("arms").map {
                    val a = it.obj("arm")
                    ArmL(
                        a.str("name"),
                        a.long("index"),
                        a.bool("fallback"),
                        a.list("payload").map { p -> Repr.from(p.obj("payload")) },
                        a.list("slots").map { s -> (s as Long).toInt() },
                    )
                },
            )
        }
    }
}
