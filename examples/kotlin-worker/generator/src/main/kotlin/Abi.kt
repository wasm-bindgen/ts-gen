package tsgen.kotlin

/** The ABI as ABI.md describes it: owners, functions and handlers. */
class Abi(val version: Int, val owners: List<Owner>, val functions: List<Func>, val handlers: List<Handler>)

/** An owner's identity: `"Path"` is global, `"Path" (module "spec")` a module's. */
data class OwnerId(val path: String, val module: String?)

class Owner(
    val kind: String, // class, interface, dictionary, namespace
    val path: String,
    val module: String?,
    val aliases: List<String>,
    val extends: List<OwnerId>,
    val doc: String?,
)

sealed class Head {
    data class OwnerRef(val path: String, val module: String?) : Head()
    object Str : Head()
    object Num : Head()
    object Bool : Head()
    object BigInt : Head()
}

/** `(@abi.ref head? empties*)`; absent means no head, admits neither. */
data class Ref(val head: Head?, val nullOk: Boolean, val undefinedOk: Boolean) {
    val empty: Boolean get() = nullOk || undefinedOk
    companion object { val NONE = Ref(null, false, false) }
}

/** A parameter or result: `valtype` is i32, i64, f64, externref or anyrefs. */
class Slot(val name: String?, val valtype: String, val ref: Ref)

sealed class Op {
    /** `@abi.get` / `set` / `call`; `on` is null for an instance member. */
    data class Member(val kind: String, val name: String, val on: List<String>?) : Op()
    data class New(val path: List<String>) : Op()
    object Object : Op()
    object IsUndefined : Op()
    object Number : Op()
    object Undefined : Op()
}

class Func(
    val module: String,
    val name: String,
    val op: Op?, // null for an alias
    val alias: String?,
    val owner: String?,
    val ts: String?,
    val doc: String?,
    val params: List<Slot>,
    val result: Slot?,
) {
    val variadic: Boolean get() = params.any { it.valtype == "anyrefs" }
}

class Handler(
    val export: String,
    val provide: List<String>,
    val promise: Boolean,
    val ts: String?,
    val doc: String?,
    val params: List<Slot>,
    val result: Slot?,
)

object AbiReader {
    private val ownerKinds = mapOf(
        "@abi.class" to "class", "@abi.interface" to "interface",
        "@abi.dictionary" to "dictionary", "@abi.namespace" to "namespace",
    )

    fun read(text: String): Abi {
        val top = SxReader(text).readAll().singleOrNull() as? Sx.Lst ?: fail("expected one (module ...)")
        if (top.head != "module") fail("expected (module ...)")
        val fields = top.rest
        val ver = (fields.firstOrNull() as? Sx.Lst)?.takeIf { it.head == "@abi.version" }
            ?: fail("the module's first field is not (@abi.version n); ts-gen --target wat output predates ABI.md")
        val version = (ver.items[1] as Sx.Atom).text.toInt()
        if (version != 1) fail("unsupported @abi.version $version")
        val owners = mutableListOf<Owner>()
        val funcs = mutableListOf<Func>()
        val handlers = mutableListOf<Handler>()
        for (f in fields.drop(1)) {
            if (f !is Sx.Lst) continue
            val head = f.head ?: continue
            when {
                head in ownerKinds -> owners += Owner(
                    kind = ownerKinds.getValue(head),
                    path = f.str(1),
                    module = f.find("module")?.str(1),
                    aliases = f.findAll("alias").map { it.str(1) },
                    extends = f.findAll("extends").map { OwnerId(it.str(1), it.find("module")?.str(1)) },
                    doc = f.find("doc")?.str(1),
                )
                head == "import" -> {
                    val module = f.str(1)
                    if (module == "./builtins.js") continue
                    val fn = f.find("func") ?: continue
                    funcs += readFunc(module, f.str(2), fn)
                }
                head == "func" -> {
                    val export = f.find("export")?.str(1) ?: continue
                    val provide = f.find("@abi.provide") ?: continue
                    handlers += Handler(
                        export = export,
                        provide = provide.rest.map { (it as Sx.Str).value },
                        promise = f.find("@abi.promise") != null,
                        ts = f.find("@abi.ts")?.str(1),
                        doc = f.find("@abi.doc")?.str(1),
                        params = f.findAll("param").map(::readSlot),
                        result = f.find("result")?.let(::readSlot),
                    )
                }
                // type $anyrefs, unknown annotations: nothing to read.
            }
        }
        return Abi(version, owners, funcs, handlers)
    }

    private fun readFunc(module: String, name: String, fn: Sx.Lst): Func {
        val alias = fn.find("@abi.alias")?.str(1)
        var op: Op? = null
        for (a in fn.lists()) {
            val o = when (a.head) {
                "@abi.get", "@abi.set", "@abi.call" ->
                    Op.Member(a.head!!.removePrefix("@abi."), a.str(1), a.find("on")?.rest?.map { (it as Sx.Str).value })
                "@abi.new" -> Op.New(a.rest.map { (it as Sx.Str).value })
                "@abi.object" -> Op.Object
                "@abi.is-undefined" -> Op.IsUndefined
                "@abi.number" -> Op.Number
                "@abi.undefined" -> Op.Undefined
                else -> null
            } ?: continue
            if (op != null) fail("$module $name: more than one operation")
            op = o
        }
        // ABI.md: an unknown operation must be rejected, but an unknown
        // operation and an unknown informational annotation look alike, so
        // the only test available is "no operation we know".
        if (op == null && alias == null) fail("$module $name: no known operation (annotations: ${fn.lists().mapNotNull { it.head }.filter { it.startsWith("@abi.") }})")
        return Func(
            module = module, name = name, op = op, alias = alias,
            owner = fn.find("@abi.owner")?.str(1),
            ts = fn.find("@abi.ts")?.str(1),
            doc = fn.find("@abi.doc")?.str(1),
            params = fn.findAll("param").map(::readSlot),
            result = fn.find("result")?.let(::readSlot),
        )
    }

    private fun readSlot(l: Sx.Lst): Slot {
        // (param $id valtype ref?) / (result valtype ref?)
        val parts = l.rest.toMutableList()
        val name = (parts.firstOrNull() as? Sx.Atom)?.text?.takeIf { it.startsWith("$") }?.removePrefix("$")
        if (name != null) parts.removeAt(0)
        val vt = parts.firstOrNull() ?: fail("no type in $l")
        val valtype = when {
            vt is Sx.Atom && vt.text in setOf("i32", "i64", "f64", "externref") -> vt.text
            vt is Sx.Lst && vt.head == "ref" -> "anyrefs"
            else -> fail("unsupported value type in $l")
        }
        val ref = (parts.getOrNull(1) as? Sx.Lst)?.takeIf { it.head == "@abi.ref" }?.let { r ->
            var head: Head? = null
            var n = false
            var u = false
            for (x in r.rest) when {
                x is Sx.Str -> head = Head.OwnerRef(x.value, null)
                x is Sx.Lst && x.head == "module" && head is Head.OwnerRef ->
                    head = (head as Head.OwnerRef).copy(module = x.str(1))
                x is Sx.Atom && x.text == "null" -> n = true
                x is Sx.Atom && x.text == "undefined" -> u = true
                x is Sx.Atom && x.text == "string" -> head = Head.Str
                x is Sx.Atom && x.text == "number" -> head = Head.Num
                x is Sx.Atom && x.text == "boolean" -> head = Head.Bool
                x is Sx.Atom && x.text == "bigint" -> head = Head.BigInt
            }
            Ref(head, n, u)
        } ?: Ref.NONE
        return Slot(name, valtype, ref)
    }
}
