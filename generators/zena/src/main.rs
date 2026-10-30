//! `ts-gen-zena <template.wat> -o <bindings.zena>`: Zena bindings for the
//! ABI template `ts-gen --target wat` writes. With handlers in the
//! template, `main.zena` (the compile entry) is written next to the
//! output, and an `app.zena` scaffold unless one exists.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};

fn main() -> Result<()> {
    let mut args = std::env::args_os().skip(1);
    let mut template = None;
    let mut output = None;
    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("-o" | "--output") => output = args.next().map(PathBuf::from),
            Some("-h" | "--help") => {
                println!("usage: ts-gen-zena <template.wat> -o <bindings.zena>");
                return Ok(());
            }
            _ if template.is_none() => template = Some(PathBuf::from(arg)),
            _ => bail!("unexpected argument {arg:?}"),
        }
    }
    let (Some(template), Some(output)) = (template, output) else {
        bail!("usage: ts-gen-zena <template.wat> -o <bindings.zena>");
    };
    let text = std::fs::read_to_string(&template)
        .with_context(|| format!("Failed to read {}", template.display()))?;
    let abi = ts_gen_abi::format::parse(&text)
        .map_err(|e| anyhow::anyhow!("{}: {e}", template.display()))?;
    let file = output
        .file_name()
        .and_then(|f| f.to_str())
        .with_context(|| format!("{} has no file name", output.display()))?;
    if !abi.handlers.is_empty() && matches!(file, "main.zena" | "app.zena") {
        bail!(
            "-o {file}: a template with handlers writes main.zena and app.zena next to the output"
        );
    }
    let zena = ts_gen_zena::generate(&abi, file)?;
    if let Some((main, app)) = &zena.provides {
        let dir = output.parent().map(Path::to_path_buf).unwrap_or_default();
        write(&dir.join("main.zena"), main)?;
        let app_file = dir.join("app.zena");
        if app_file.exists() {
            eprintln!("[ts-gen-zena] Kept existing {}", app_file.display());
        } else {
            write(&app_file, app)?;
        }
    }
    write(&output, &zena.bindings)
}

fn write(path: &Path, contents: &str) -> Result<()> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create {}", parent.display()))?;
    }
    std::fs::write(path, contents)
        .with_context(|| format!("Failed to write {}", path.display()))?;
    eprintln!("[ts-gen-zena] Wrote {}", path.display());
    Ok(())
}
