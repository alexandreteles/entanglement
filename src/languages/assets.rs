use std::{fs, path::PathBuf, sync::OnceLock};

use crate::Result;

include!(concat!(env!("OUT_DIR"), "/grammar_assets.rs"));

static GRAMMAR_ROOT: OnceLock<std::result::Result<PathBuf, String>> = OnceLock::new();

/// Put the embedded upstream grammar in the Tree-sitter cache.
///
/// Use a content hash to keep different grammar versions separate. Return an
/// error if the cache cannot be read or written. Source files stay unchanged.
pub fn prepare(loader: &tree_sitter_loader::Loader) -> Result<PathBuf> {
    GRAMMAR_ROOT
        .get_or_init(|| {
            let mut hasher = blake3::Hasher::new();
            for (grammar, name, bytes) in GRAMMAR_ASSETS {
                hasher.update(grammar.as_bytes());
                hasher.update(name.as_bytes());
                hasher.update(bytes);
            }
            let root = loader
                .parser_lib_path
                .join("entanglement")
                .join(hasher.finalize().to_hex().as_str());
            for (grammar, name, bytes) in GRAMMAR_ASSETS {
                let path = root.join(grammar).join(name);
                if fs::read(&path).is_ok_and(|existing| existing == *bytes) {
                    continue;
                }
                fs::create_dir_all(path.parent().ok_or("No asset directory")?)
                    .map_err(|error| error.to_string())?;
                let temporary = path.with_extension(format!("{}.tmp", std::process::id()));
                fs::write(&temporary, bytes).map_err(|error| error.to_string())?;
                fs::rename(&temporary, &path).map_err(|error| error.to_string())?;
            }
            Ok(root)
        })
        .clone()
        .map_err(Into::into)
}
