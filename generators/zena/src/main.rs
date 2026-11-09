use std::path::PathBuf;

use anyhow::{bail, Context};

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let mut input = None;
    let mut output = None;
    let mut opts = ts_gen_zena::Options::default();
    let mut entry = None;
    let mut libs: Vec<(PathBuf, String)> = Vec::new();
    while let Some(a) = args.next() {
        match a.as_str() {
            "-o" | "--output" => output = args.next().map(PathBuf::from),
            "--probe" => opts.probe = true,
            "--entry" => {
                let (Some(e), Some(bindings), Some(app)) = (args.next(), args.next(), args.next())
                else {
                    bail!("--entry takes <entry.zena> <bindings specifier> <app specifier>");
                };
                entry = Some(PathBuf::from(e));
                opts.entry = Some(ts_gen_zena::Entry { bindings, app });
            }
            "--with" => {
                let (Some(ir), Some(module)) = (args.next(), args.next()) else {
                    bail!("--with takes <lib.ir> <bindings specifier>");
                };
                libs.push((PathBuf::from(ir), module));
            }
            "-h" | "--help" => {
                println!("usage: ts-gen-zena <input.ir> [-o <out.zena>] [--probe] [--entry <entry.zena> <bindings> <app>] [--with <lib.ir> <lib bindings>]");
                return Ok(());
            }
            _ if input.is_none() => input = Some(PathBuf::from(a)),
            _ => bail!("unexpected argument `{a}`"),
        }
    }
    let Some(input) = input else {
        bail!("usage: ts-gen-zena <input.ir> [-o <out.zena>] [--probe] [--entry <entry.zena> <bindings> <app>] [--with <lib.ir> <lib bindings>]");
    };
    let src = std::fs::read_to_string(&input).with_context(|| input.display().to_string())?;
    let ir =
        ts_gen_abi::text::parse(&src).map_err(|e| anyhow::anyhow!("{}: {e}", input.display()))?;
    let mut lib_irs = Vec::new();
    for (path, module) in &libs {
        let src = std::fs::read_to_string(path).with_context(|| path.display().to_string())?;
        let lib = ts_gen_abi::text::parse(&src)
            .map_err(|e| anyhow::anyhow!("{}: {e}", path.display()))?;
        lib_irs.push((lib, module.clone()));
    }
    let libs: Vec<ts_gen_zena::Lib> = lib_irs
        .iter()
        .map(|(ir, module)| ts_gen_zena::Lib {
            ir,
            module: module.clone(),
        })
        .collect();
    let out = ts_gen_zena::generate_with(&ir, &libs, &opts)?;
    for s in &out.skipped {
        eprintln!("skipped {}: {}", s.key, s.reason);
    }
    if let (Some(p), Some(src)) = (&entry, &out.entry) {
        std::fs::write(p, src).with_context(|| p.display().to_string())?;
    }
    match output {
        Some(p) => std::fs::write(&p, out.zena).with_context(|| p.display().to_string())?,
        None => print!("{}", out.zena),
    }
    Ok(())
}
