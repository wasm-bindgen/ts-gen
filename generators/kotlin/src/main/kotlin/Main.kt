package tsgen.kotlin

import java.io.File
import kotlin.system.exitProcess

const val USAGE = """usage: ts-gen-kotlin <lowered.json> <out-dir> [--package <pkg>] [--app-package <pkg>] [--lib <name>=<pkg>]... [--runtime] [--scaffold <file>]

Writes Kotlin/Wasm bindings for a lowered IR (`ts-gen-abi lower --profile
"all-host, results = single, throws = js"`):
  <out-dir>/<pkg>/flat/Flat.kt  one external or export per member, by wire
  <out-dir>/<pkg>/Bindings.kt   the typed API (default package: workers)
  <out-dir>/<pkg>/Vecs.kt       the typed arrays the API uses
  <out-dir>/<pkg>/Tuples.kt     the tuples other than pairs and triples
  <out-dir>/<pkg>/Exports.kt    exports calling <app-package> (default: app)
  <out-dir>/links.tsv           js_code imports to rename to their keys
  <out-dir>/tsgen/runtime/Runtime.kt  with --runtime: futures and the loop,
                                once per program
<out-dir> is replaced. --lib names the package a lib's bindings were
generated into, for each lib the IR resolves against. --scaffold writes
the declarations the exports call, with TODO() bodies, to <file> if it
does not exist."""

/**
 * Kotlin/Wasm interop can name no GC type, so every category is `host`;
 * Kotlin functions return one value, so results spill; Kotlin can't
 * declare Wasm tags, so errors cross as JS exceptions; and Kotlin reads
 * `undefined` as null, so a box's none is `ref.null`.
 */
private val PROFILE = mapOf(
    "vec" to "host", "array" to "host", "str" to "host", "handles" to "host",
    "results" to "single", "throws" to "js", "box-none" to "null",
)

fun checkProfile(p: Map<String, String>): String? {
    val bad = PROFILE.filter { (k, v) -> p[k] != v }.keys
    return if (bad.isEmpty()) null
    else "Kotlin needs `all-host, results = single, throws = js, box-none = null`; ${bad.joinToString { "$it = ${p[it]}" }}"
}

/** The protocol imports of `tsgen.runtime`. */
val RUNTIME_LINKS = listOf(
    "tsgen.runtime.futurePoll" to Import("tw:future", "poll"),
    "tsgen.runtime.futureDrop" to Import("tw:future", "drop"),
    "tsgen.runtime.loopWorkAvailable" to Import("tw:loop", "work_available"),
)

fun main(args: Array<String>) {
    val pos = mutableListOf<String>()
    var pkg = "workers"
    var appPkg = "app"
    var scaffold: String? = null
    var runtime = false
    val libPkgs = HashMap<String, String>()
    var i = 0
    while (i < args.size) {
        when (args[i]) {
            "--package" -> pkg = args[++i]
            "--app-package" -> appPkg = args[++i]
            "--scaffold" -> scaffold = args[++i]
            "--runtime" -> runtime = true
            "--lib" -> args[++i].split('=', limit = 2).let { if (it.size == 2) libPkgs[it[0]] = it[1] else { System.err.println(USAGE); exitProcess(2) } }
            "-h", "--help" -> { println(USAGE); return }
            else -> pos += args[i]
        }
        i++
    }
    if (pos.size != 2) { System.err.println(USAGE); exitProcess(2) }
    val low = try {
        Lowered.read(File(pos[0]).readText())
    } catch (e: JsonError) {
        System.err.println("error: ${pos[0]}: ${e.message}"); exitProcess(1)
    }
    checkProfile(low.profile)?.let { System.err.println("error: ${pos[0]}: $it"); exitProcess(1) }
    low.libs.firstOrNull { it.name !in libPkgs }?.let {
        System.err.println("error: ${pos[0]}: resolves against the lib `${it.name}`; name its package with --lib ${it.name}=<pkg>"); exitProcess(1)
    }
    val out = Gen(low, pkg, appPkg, libPkgs).run()
    val dir = File(pos[1])
    dir.deleteRecursively()
    for ((rel, text) in out.files) File(dir, rel).apply { parentFile.mkdirs(); writeText(text) }
    val links = out.links.toMutableList()
    if (runtime) {
        File(dir, "tsgen/runtime/Runtime.kt").apply { parentFile.mkdirs(); writeText(Gen::class.java.getResource("/Runtime.kt")!!.readText()) }
        links += RUNTIME_LINKS
    }
    File(dir, "links.tsv").writeText(links.joinToString("") { (from, to) -> "js_code\t$from\t${to.module}\t${to.name}\n" })
    if (out.skipped.isNotEmpty()) {
        File(dir, "skipped.txt").writeText(out.skipped.joinToString("") { "$it\n" })
        System.err.println("skipped ${out.skipped.size} members (see ${File(dir, "skipped.txt")})")
    }
    if (out.warnings.isNotEmpty()) {
        File(dir, "warnings.txt").writeText(out.warnings.joinToString("") { "$it\n" })
        for (w in out.warnings) System.err.println("warning: $w")
    }
    scaffold?.let(::File)?.takeIf { !it.exists() && out.app != null }?.let { f ->
        f.parentFile?.mkdirs()
        f.writeText(out.app!!)
    }
}
