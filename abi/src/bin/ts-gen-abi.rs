//! `ts-gen-abi lower <file.ir> [--profile <profile>] [--with <lib.ir>]...`:
//! the lowered IR as JSON on stdout ([`ts_gen_abi::json`]). `<profile>` is a
//! file holding the profile text form, or the text itself (`all-host`).
//! `--with` supplies the lib IRs its `with` names (§5).

use std::process::ExitCode;

use ts_gen_abi::profile::Profile;
use ts_gen_abi::{json, resolve, text, validate};

const USAGE: &str =
    "usage: ts-gen-abi lower <file.ir> [--profile <file or text>] [--with <lib.ir>]...";

fn run(args: &[String]) -> Result<String, String> {
    let (cmd, rest) = args.split_first().ok_or(USAGE)?;
    if cmd != "lower" {
        return Err(USAGE.into());
    }
    let mut file = None;
    let mut profile = Profile::default();
    let mut supplied = Vec::new();
    let mut it = rest.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--profile" => {
                let v = it.next().ok_or(USAGE)?;
                let src = std::fs::read_to_string(v).unwrap_or_else(|_| v.clone());
                profile = Profile::parse(&src).map_err(|e| format!("{v}: {e}"))?;
            }
            "--with" => {
                let v = it.next().ok_or(USAGE)?;
                let src = std::fs::read_to_string(v).map_err(|e| format!("{v}: {e}"))?;
                supplied.push(text::parse(&src).map_err(|e| format!("{v}: {e}"))?);
            }
            _ if file.is_none() => file = Some(a),
            _ => return Err(USAGE.into()),
        }
    }
    let file = file.ok_or(USAGE)?;
    let src = std::fs::read_to_string(file).map_err(|e| format!("{file}: {e}"))?;
    let ir = text::parse(&src).map_err(|e| format!("{file}: {e}"))?;
    let opts = validate::Options {
        libs: &supplied,
        ..Default::default()
    };
    validate::validate_with(&ir, opts).map_err(|es| {
        es.iter()
            .map(|e| format!("{file}: {e}"))
            .collect::<Vec<_>>()
            .join("\n")
    })?;
    let libs = resolve::libs_for(&ir, &supplied).unwrap_or_default();
    Ok(json::lower_with(&ir, &libs, &profile).pretty())
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(out) => {
            print!("{out}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}
