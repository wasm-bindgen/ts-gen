//! Language generators. `--lang <L>` runs the `ts-gen-<L>` binary, which
//! is found through `TS_GEN_<L>`, then in a cache private to this version
//! of `ts-gen`, and is otherwise installed there with `cargo install`.
//! `PATH` isn't searched, so a generator always matches the `ts-gen`
//! running it unless the variable says otherwise.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{bail, Context, Result};

const VERSION: &str = env!("CARGO_PKG_VERSION");

pub fn check_lang(lang: &str) -> Result<()> {
    let ok = |b: u8| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-';
    if lang.is_empty() || !lang.bytes().all(ok) {
        bail!("--lang {lang:?}: a language name holds lowercase letters, digits and `-`");
    }
    Ok(())
}

/// The variable naming a generator's binary: `TS_GEN_WASM_BINDGEN` for
/// `wasm-bindgen`.
pub fn env_var(lang: &str) -> String {
    format!("TS_GEN_{}", lang.to_ascii_uppercase().replace('-', "_"))
}

/// The per-user cache directory, read through `var`.
fn cache_dir(var: impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    let dir = |name: &str| var(name).map(PathBuf::from).filter(|p| p.is_absolute());
    if cfg!(windows) {
        dir("LOCALAPPDATA")
    } else if cfg!(target_os = "macos") {
        dir("HOME").map(|h| h.join("Library/Caches"))
    } else {
        dir("XDG_CACHE_HOME").or_else(|| dir("HOME").map(|h| h.join(".cache")))
    }
}

/// The `cargo install --root` this version's generators share.
fn install_root(cache: &Path) -> PathBuf {
    cache.join("ts-gen").join(VERSION)
}

fn installed(root: &Path, lang: &str) -> PathBuf {
    root.join("bin")
        .join(format!("ts-gen-{lang}{}", std::env::consts::EXE_SUFFIX))
}

/// The generator is pinned to this version, so the two always agree on
/// the options passed between them.
fn install_args(lang: &str, root: &Path) -> Vec<OsString> {
    vec![
        "install".into(),
        format!("ts-gen-{lang}").into(),
        "--version".into(),
        format!("={VERSION}").into(),
        "--locked".into(),
        "--root".into(),
        root.into(),
    ]
}

fn resolve(lang: &str) -> Result<PathBuf> {
    let var = env_var(lang);
    if let Some(path) = std::env::var_os(&var).filter(|v| !v.is_empty()) {
        return Ok(path.into());
    }
    let Some(cache) = cache_dir(|name| std::env::var_os(name)) else {
        bail!("no cache directory to install ts-gen-{lang} into; set {var} to its binary");
    };
    let root = install_root(&cache);
    let bin = installed(&root, lang);
    if bin.is_file() {
        return Ok(bin);
    }
    eprintln!(
        "[ts-gen] Installing ts-gen-{lang} {VERSION} into {}",
        root.display()
    );
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    // The generator may write its output to stdout.
    let status = Command::new(&cargo)
        .args(install_args(lang, &root))
        .stdout(Stdio::from(std::io::stderr()))
        .status()
        .with_context(|| {
            format!(
                "couldn't run {} to install ts-gen-{lang}; set {var} to its binary",
                Path::new(&cargo).display()
            )
        })?;
    if !status.success() || !bin.is_file() {
        bail!("couldn't install ts-gen-{lang} {VERSION}; set {var} to its binary");
    }
    Ok(bin)
}

/// Runs the generator on `args`, with this process's stdio, and returns
/// its exit code.
pub fn run(lang: &str, args: &[OsString]) -> Result<i32> {
    let bin = resolve(lang)?;
    let status = Command::new(&bin)
        .args(args)
        .status()
        .with_context(|| format!("couldn't run {}", bin.display()))?;
    Ok(status.code().unwrap_or(1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lang_names() {
        for ok in ["wasm-bindgen", "kotlin", "c99"] {
            check_lang(ok).unwrap();
        }
        for bad in ["", "Rust", "a_b", "../x", "a b"] {
            assert!(check_lang(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn env_vars() {
        assert_eq!(env_var("wasm-bindgen"), "TS_GEN_WASM_BINDGEN");
        assert_eq!(env_var("kotlin"), "TS_GEN_KOTLIN");
    }

    #[test]
    fn cache_paths() {
        let env = |pairs: &[(&'static str, &str)]| {
            let pairs: Vec<(&str, OsString)> = pairs.iter().map(|(k, v)| (*k, v.into())).collect();
            move |name: &str| {
                pairs
                    .iter()
                    .find(|(k, _)| *k == name)
                    .map(|(_, v)| v.clone())
            }
        };
        let abs = |p: &str| match cfg!(windows) {
            true => format!("C:\\{p}"),
            false => format!("/{p}"),
        };
        let (home, xdg, local) = (abs("home"), abs("xdg"), abs("local"));
        let all = [
            ("HOME", &*home),
            ("XDG_CACHE_HOME", &*xdg),
            ("LOCALAPPDATA", &*local),
        ];
        let want = if cfg!(windows) {
            PathBuf::from(&local)
        } else if cfg!(target_os = "macos") {
            Path::new(&home).join("Library/Caches")
        } else {
            PathBuf::from(&xdg)
        };
        assert_eq!(cache_dir(env(&all)), Some(want));
        assert_eq!(cache_dir(env(&[])), None);
        // A relative directory is ignored, as the XDG spec has it.
        assert_eq!(cache_dir(env(&[("XDG_CACHE_HOME", "cache")])), None);
        if cfg!(all(unix, not(target_os = "macos"))) {
            assert_eq!(
                cache_dir(env(&[("HOME", "/home")])),
                Some(PathBuf::from("/home/.cache"))
            );
        }
    }

    #[test]
    fn install() {
        let root = install_root(Path::new("cache"));
        assert_eq!(root, Path::new("cache").join("ts-gen").join(VERSION));
        let bin = installed(&root, "kotlin");
        assert_eq!(
            bin,
            root.join("bin")
                .join(format!("ts-gen-kotlin{}", std::env::consts::EXE_SUFFIX))
        );
        let args: Vec<String> = install_args("kotlin", &root)
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            args,
            [
                "install",
                "ts-gen-kotlin",
                "--version",
                &format!("={VERSION}"),
                "--locked",
                "--root",
                &root.to_string_lossy(),
            ]
        );
    }
}
