# kotlin-bindgen

An example language generator over the ts-gen IR, written in Kotlin. It
reads `ts-gen --json`'s output under the profile `all-host, results =
single, throws = js` and writes Kotlin/Wasm externals. Kotlin/Wasm names
its own imports, so it also writes a `links.tsv`, and its `link` command
renames the compiled module's imports to the keys the bridge reads.

It is an example of a language project built on ts-gen, not a language
target of ts-gen: the lowered JSON is all it reads, and `ts-gen --bridge`
supplies the JS side of the module it binds.

```sh
./gradlew installDist   # JDK 21
kotlin-bindgen <lowered.json> <out-dir> [--package <pkg>] [--app-package <pkg>] [--lib <name>=<pkg>]... [--runtime]
kotlin-bindgen link <in.wasm> <links.tsv>... -o <out.wasm>
```

[example](example) is a Cloudflare Worker written in Kotlin over the
generator's output for workers-types.

`just test-kotlin` runs the tests (`tests/kotlin.rs`): snapshots of every
fixture, Kotlin/Wasm compiles that check each module's imports and exports
against the lowered layouts, and the example under Node.
