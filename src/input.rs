//! Reading `.d.ts` inputs: what every front end does before it lowers or
//! generates.

use std::path::PathBuf;

use anyhow::{bail, Result};
use walkdir::WalkDir;

use crate::context::GlobalContext;
use crate::external_map::ExternalMap;
use crate::ir::Module;

/// The `.d.ts` files `paths` name: each file as given, and every `.d.ts`
/// under each directory.
pub fn resolve_input_files(paths: &[PathBuf]) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for path in paths {
        if path.is_dir() {
            for entry in WalkDir::new(path) {
                let entry = entry?;
                if entry.file_type().is_file()
                    && entry.file_name().to_string_lossy().ends_with(".d.ts")
                {
                    files.push(entry.into_path());
                }
            }
        } else if path.is_file() {
            files.push(path.clone());
        } else {
            bail!("Input path does not exist: {}", path.display());
        }
    }
    Ok(files)
}

/// How [`load`] reads its inputs.
pub struct Load<'a> {
    /// The JS module specifier of an input with top-level exports.
    pub lib_name: Option<&'a str>,
    /// Side `.d.ts` files or directories whose `@tw-abi` and `@throws`
    /// tags are layered onto the inputs' declarations.
    pub annotate: &'a [PathBuf],
}

/// Parses the inputs and applies `--annotate`, printing the diagnostics.
/// `externals` sets up the external map after the parse and before the
/// annotations are applied.
pub fn load(
    files: &[PathBuf],
    opts: &Load,
    externals: impl FnOnce(&mut ExternalMap),
) -> Result<(Module, GlobalContext)> {
    let (module, mut gctx) = crate::parse(files, opts.lib_name)?;
    externals(&mut gctx.external_map);

    // A diagnostic the inputs and the side files both report prints once.
    let mut seen = Default::default();
    if !opts.annotate.is_empty() {
        let files = resolve_input_files(opts.annotate)?;
        let (annotations, agctx) = crate::parse(&files, opts.lib_name)?;
        agctx.diagnostics.emit_unseen(&mut seen);
        if agctx.diagnostics.has_errors() {
            let files: Vec<String> = opts
                .annotate
                .iter()
                .map(|p| p.display().to_string())
                .collect();
            bail!("--annotate: invalid tags in {}", files.join(", "));
        }
        let errors = crate::parse::annotate::apply(&mut gctx, &module, &annotations, &agctx);
        if !errors.is_empty() {
            bail!("--annotate:\n  {}", errors.join("\n  "));
        }
    }

    gctx.diagnostics.emit_unseen(&mut seen);
    Ok((module, gctx))
}
