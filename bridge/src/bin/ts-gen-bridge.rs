//! `ts-gen-bridge`: the bridge generator and the link step, for languages
//! built outside Rust.
//!
//! ```sh
//! ts-gen-bridge link <in.wasm> <links.tsv>... -o <out.wasm>
//! ts-gen-bridge generate <file.ir> <language.wasm> -o <dir> [--profile <profile>] [--with <lib.ir>]... [--wat]
//! ```
//!
//! `link` renames the imports the links files list ([`ts_gen_bridge::link`]).
//! `generate` writes `<dir>/bridge.js` and `<dir>/bridge.wasm` for the
//! (linked) language module, and `<dir>/bridge.wat` with `--wat`. The
//! language module may be `.wat` text. `<profile>` is a file holding the
//! profile text form, or the text itself (`all-host`, `"all-host, results =
//! single"`).

use std::process::ExitCode;

use ts_gen_abi::profile::Profile;
use ts_gen_abi::text;
use ts_gen_bridge::link;

const USAGE: &str = "usage: ts-gen-bridge link <in.wasm> <links.tsv>... -o <out.wasm>
       ts-gen-bridge generate <file.ir> <language.wasm> -o <dir> [--profile <profile>] [--with <lib.ir>]... [--wat]";

fn read(p: &str) -> Result<Vec<u8>, String> {
    std::fs::read(p).map_err(|e| format!("{p}: {e}"))
}

/// A module as binary, or assembled from `.wat` text.
fn read_module(p: &str) -> Result<Vec<u8>, String> {
    if p.ends_with(".wat") {
        return wat::parse_file(p).map_err(|e| format!("{p}: {e}"));
    }
    read(p)
}

fn read_str(p: &str) -> Result<String, String> {
    std::fs::read_to_string(p).map_err(|e| format!("{p}: {e}"))
}

fn write(p: &std::path::Path, bytes: &[u8]) -> Result<(), String> {
    std::fs::write(p, bytes).map_err(|e| format!("{}: {e}", p.display()))
}

fn run(args: &[String]) -> Result<(), String> {
    let (cmd, rest) = args.split_first().ok_or(USAGE)?;
    let mut pos = Vec::new();
    let mut out = None;
    let mut profile = Profile::default();
    let mut libs = Vec::new();
    let mut emit_wat = false;
    let mut it = rest.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "-o" => out = Some(it.next().ok_or(USAGE)?.clone()),
            "--profile" if cmd == "generate" => {
                let v = it.next().ok_or(USAGE)?;
                let src = std::fs::read_to_string(v).unwrap_or_else(|_| v.clone());
                profile = Profile::parse(&src).map_err(|e| format!("{v}: {e}"))?;
            }
            "--wat" if cmd == "generate" => emit_wat = true,
            "--with" if cmd == "generate" => {
                let v = it.next().ok_or(USAGE)?;
                libs.push(text::parse(&read_str(v)?).map_err(|e| format!("{v}: {e}"))?);
            }
            _ => pos.push(a.as_str()),
        }
    }
    let out = out.ok_or(USAGE)?;
    match (cmd.as_str(), &pos[..]) {
        ("link", [wasm, files @ ..]) if !files.is_empty() => {
            let mut links = link::Links::new();
            for f in files {
                let more = link::read_links(&read_str(f)?).map_err(|e| format!("{f}: {e}"))?;
                links.extend(more);
            }
            let linked =
                link::link(&read_module(wasm)?, &links).map_err(|e| format!("{wasm}: {e}"))?;
            write(out.as_ref(), &linked)
        }
        ("generate", [ir, wasm]) => {
            let ir = text::parse(&read_str(ir)?).map_err(|e| format!("{ir}: {e}"))?;
            let b = ts_gen_bridge::generate_with(&ir, &libs, &profile, &read_module(wasm)?)
                .map_err(|e| e.to_string())?;
            for w in &b.warnings {
                eprintln!("warning: {w}");
            }
            let dir = std::path::Path::new(&out);
            std::fs::create_dir_all(dir).map_err(|e| format!("{out}: {e}"))?;
            write(&dir.join("bridge.js"), b.js.as_bytes())?;
            if emit_wat {
                write(&dir.join("bridge.wat"), b.wat.as_bytes())?;
            }
            write(&dir.join("bridge.wasm"), &b.wasm)
        }
        _ => Err(USAGE.into()),
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}
