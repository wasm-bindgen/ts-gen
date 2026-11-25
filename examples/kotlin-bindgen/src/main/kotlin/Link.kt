package tsgen.kotlin

import java.io.ByteArrayOutputStream

class LinkError(message: String) : Exception(message)

/** `(module, name)` as compiled, to `(module, name)` as the bridge reads it. */
typealias Links = MutableMap<Pair<String, String>, Pair<String, String>>

/**
 * Adds a `links.tsv` to [links]: per line, `<module>\t<name>\t<tw module>\t<key>`.
 * Blank lines are skipped. Keys hold no tabs or newlines, so there's no
 * escaping.
 */
fun readLinks(text: String, links: Links) {
    for ((i, line) in text.lines().withIndex()) {
        if (line.isEmpty()) continue
        val f = line.split('\t')
        if (f.size != 4) throw LinkError("links line ${i + 1}: expected 4 tab-separated fields, got ${f.size}")
        val to = f[2] to f[3]
        val prev = links.put(f[0] to f[1], to)
        if (prev != null && prev != to) {
            throw LinkError("links line ${i + 1}: \"${f[0]}\" \"${f[1]}\" links to both $prev and $to")
        }
    }
}

private class Reader(val b: ByteArray) {
    var at = 0

    fun byte(): Int {
        if (at >= b.size) throw LinkError("the module ends early")
        return b[at++].toInt() and 0xff
    }

    fun u32(): Int {
        var r = 0L
        var shift = 0
        while (true) {
            val x = byte()
            r = r or ((x and 0x7f).toLong() shl shift)
            if (x and 0x80 == 0) break
            shift += 7
            if (shift > 28) throw LinkError("a malformed LEB128 at byte $at")
        }
        if (r > Int.MAX_VALUE) throw LinkError("a size past 2 GiB at byte $at")
        return r.toInt()
    }

    /** Steps over a LEB128 of any width. */
    fun skipLeb() {
        while (byte() and 0x80 != 0) Unit
    }

    fun name(): String {
        val n = u32()
        if (n > b.size - at) throw LinkError("the module ends early")
        return String(b, at, n, Charsets.UTF_8).also { at += n }
    }

    /** `(ref ht)` and `(ref null ht)` carry a heap type; every other type is its one byte. */
    fun skipValType() {
        val t = byte()
        if (t == 0x63 || t == 0x64) skipLeb()
    }

    fun skipLimits() {
        val flags = byte()
        skipLeb()
        if (flags and 1 != 0) skipLeb()
        if (flags and 8 != 0) skipLeb()
    }

    fun skipImportType() {
        when (val kind = byte()) {
            0x00 -> skipLeb()
            0x01 -> { skipValType(); skipLimits() }
            0x02 -> skipLimits()
            0x03 -> { skipValType(); byte() }
            0x04 -> { byte(); skipLeb() }
            else -> throw LinkError("an import of kind $kind at byte ${at - 1}")
        }
    }
}

private fun leb(out: ByteArrayOutputStream, v: Int) {
    var n = v
    while (true) {
        val x = n and 0x7f
        n = n ushr 7
        if (n == 0) { out.write(x); return }
        out.write(x or 0x80)
    }
}

private fun name(out: ByteArrayOutputStream, s: String) {
    val b = s.toByteArray(Charsets.UTF_8)
    leb(out, b.size)
    out.write(b)
}

/**
 * [wasm] with each import [links] names renamed.
 *
 * Kotlin/Wasm imports every `@JsFun` from `js_code` under the function's
 * qualified name, so the generator lists each with the import it stands
 * for, and this rewrites the compiled module's import section to match
 * before the bridge reads it. Only the names change: an import's type and
 * every other section are kept byte for byte. An entry the module doesn't
 * import is ignored, since the compiler may have dropped it.
 */
fun link(wasm: ByteArray, links: Map<Pair<String, String>, Pair<String, String>>): ByteArray {
    val header = byteArrayOf(0, 0x61, 0x73, 0x6d, 1, 0, 0, 0)
    if (wasm.size < 8 || !wasm.copyOf(8).contentEquals(header)) throw LinkError("not a Wasm module")
    val r = Reader(wasm)
    r.at = 8
    val out = ByteArrayOutputStream(wasm.size + 1024)
    out.write(wasm, 0, 8)
    while (r.at < wasm.size) {
        val start = r.at
        val id = r.byte()
        val size = r.u32()
        if (size > wasm.size - r.at) throw LinkError("the module ends early")
        val end = r.at + size
        if (id != 2) {
            out.write(wasm, start, end - start)
            r.at = end
            continue
        }
        val section = ByteArrayOutputStream(size + 1024)
        val count = r.u32()
        leb(section, count)
        repeat(count) {
            val entry = r.at
            val from = r.name() to r.name()
            val type = r.at
            r.skipImportType()
            val to = links[from]
            if (to == null) {
                section.write(wasm, entry, r.at - entry)
            } else {
                name(section, to.first)
                name(section, to.second)
                section.write(wasm, type, r.at - type)
            }
        }
        if (r.at != end) throw LinkError("the import section's size doesn't match its entries")
        out.write(2)
        leb(out, section.size())
        section.writeTo(out)
    }
    return out.toByteArray()
}
