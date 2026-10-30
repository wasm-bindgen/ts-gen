package tsgen.kotlin

import java.io.File
import kotlin.system.exitProcess

const val USAGE = """usage: generator <template.abi.wat> <out-dir> [--package <pkg>] [--app-package <pkg>] [--scaffold <file>]

Writes Kotlin/Wasm bindings for a ts-gen ABI template (ABI.md):
  <out-dir>/flat/*.kt               @JsFun externals named like the ABI imports
  <out-dir>/<pkg>/Bindings.kt       typed layer (default package: workers)
  <out-dir>/<pkg>/provided/Entry.kt @JsExport handlers calling <app-package>.<name> (default: app)
<out-dir> is replaced. --scaffold writes the functions the entry calls, with
TODO() bodies, to <file> if it does not exist."""

fun main(args: Array<String>) {
    val pos = mutableListOf<String>()
    var pkg = "workers"
    var appPkg = "app"
    var scaffold: String? = null
    var i = 0
    while (i < args.size) {
        when (args[i]) {
            "--package" -> pkg = args[++i]
            "--app-package" -> appPkg = args[++i]
            "--scaffold" -> scaffold = args[++i]
            "-h", "--help" -> { println(USAGE); return }
            else -> pos += args[i]
        }
        i++
    }
    if (pos.size != 2) { System.err.println(USAGE); exitProcess(2) }
    val abi = try {
        AbiReader.read(File(pos[0]).readText())
    } catch (e: IllegalArgumentException) {
        System.err.println("error: ${pos[0]}: ${e.message}"); exitProcess(1)
    }
    val gen = Gen(abi, pkg, appPkg)
    val out = File(pos[1])
    out.deleteRecursively()
    val files = gen.flatFiles().toMutableMap()
    val pdir = pkg.replace('.', '/')
    files["$pdir/Bindings.kt"] = gen.typedFile()
    gen.entryFile()?.let { files["$pdir/provided/Entry.kt"] = it }
    for ((rel, text) in files) File(out, rel).apply { parentFile.mkdirs(); writeText(text) }
    println("wrote ${files.size} files to $out")
    scaffold?.let(::File)?.takeIf { !it.exists() && abi.handlers.isNotEmpty() }?.let { f ->
        f.parentFile?.mkdirs()
        val fns = gen.appSignatures().lines().joinToString("\n\n") { sig ->
            val name = sig.substringAfter("fun ").substringBefore('(')
            "$sig = TODO(\"$name\")"
        }
        f.writeText("package $appPkg\n\n$fns\n")
        println("wrote $f")
    }
    if (abi.handlers.isNotEmpty()) println("the entry calls, in package $appPkg:\n" + gen.appSignatures().prependIndent("  "))
}
