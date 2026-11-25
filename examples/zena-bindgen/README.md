# zena-bindgen

An example language generator over the ts-gen IR. It reads an `.tw` file and
writes Zena `@external` declarations, one per generic instantiation, under
the GC profile with host strings (`all-gc, str = host`).

It is an example of a language project built on ts-gen, not a language
target of ts-gen: the generator depends only on the `ts-gen-abi` crate, for
the IR and its layouts, and `ts-gen --bridge` supplies the JS side of the
module it binds.

```sh
zena-bindgen <input.tw> [-o <out.zena>] [--entry <entry.zena> <bindings> <app>] [--with <lib.tw> <lib bindings>]
```

The bindings need the Zena compiler fork (`guybedford/zena`) with templated
`@external` names, `@export` and `@spelling`.

[example](example) is a Cloudflare Worker written in Zena over the
generator's output for workers-types.

`just test-zena` runs the tests: snapshots of every fixture, and, with a
`zena-cli` (`ZENA`, `ZENA_REPO_ROOT`), compiles that check each module's
imports and exports against the IR's.
