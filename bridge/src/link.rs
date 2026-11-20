//! Renames a language module's imports to their `tw` keys.
//!
//! Some languages can't choose an import's `(module, name)`: Kotlin/Wasm
//! imports every `@JsFun` from `js_code` under the function's qualified
//! name. Their generator lists each such import with the key it stands for
//! in `links.tsv`, and [`link`] rewrites the module's import section to
//! match, before the bridge reads it. Only the names change; types, indices
//! and every other section are kept byte for byte.

use std::collections::BTreeMap;

use crate::{Error, Result};

/// `(module, name)` as compiled, to `(module, name)` as the bridge reads it.
pub type Links = BTreeMap<(String, String), (String, String)>;

/// Reads `links.tsv`: per line, `<module>\t<name>\t<tw module>\t<key>`.
/// Blank lines are skipped. Keys hold no tabs or newlines, so there's no
/// escaping.
pub fn read_links(text: &str) -> Result<Links> {
    let mut links = Links::new();
    for (i, line) in text.lines().enumerate() {
        if line.is_empty() {
            continue;
        }
        let f: Vec<&str> = line.split('\t').collect();
        let [m, n, tm, tn] = f[..] else {
            return Err(Error::Module(format!(
                "links line {}: expected 4 tab-separated fields, got {}",
                i + 1,
                f.len()
            )));
        };
        let to = (tm.to_string(), tn.to_string());
        if let Some(prev) = links.insert((m.to_string(), n.to_string()), to.clone()) {
            if prev != to {
                return Err(Error::Module(format!(
                    "links line {}: {m:?} {n:?} links to both {prev:?} and {to:?}",
                    i + 1
                )));
            }
        }
    }
    Ok(links)
}

/// `wasm` with each import `links` names renamed. An entry the module
/// doesn't import is ignored: the compiler may have dropped it.
pub fn link(wasm: &[u8], links: &Links) -> Result<Vec<u8>> {
    use wasm_encoder::reencode::{Reencode, RoundtripReencoder};
    use wasm_encoder::RawSection;
    use wasmparser::Payload;

    let err = |e: wasmparser::BinaryReaderError| Error::Module(e.to_string());
    let mut out = wasm_encoder::Module::new();
    for payload in wasmparser::Parser::new(0).parse_all(wasm) {
        let payload = payload.map_err(err)?;
        match &payload {
            Payload::ImportSection(reader) => {
                let mut imports = wasm_encoder::ImportSection::new();
                for import in reader.clone().into_imports() {
                    let import = import.map_err(err)?;
                    let key = (import.module.to_string(), import.name.to_string());
                    let (module, name) = links.get(&key).unwrap_or(&key);
                    let ty = RoundtripReencoder
                        .entity_type(import.ty)
                        .map_err(|e| Error::Module(format!("import {module:?} {name:?}: {e:?}")))?;
                    imports.import(module, name, ty);
                }
                out.section(&imports);
            }
            _ => {
                if let Some((id, range)) = payload.as_section() {
                    out.section(&RawSection {
                        id,
                        data: &wasm[range.start as usize..range.end as usize],
                    });
                }
            }
        }
    }
    Ok(out.finish())
}
