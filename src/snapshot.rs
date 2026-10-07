//! A run-local file view for configuration read during resolution.
//! Clones retain observed bytes/errors; candidate edits overlay only the after view.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use crate::Result;

type Contents = std::result::Result<Option<Arc<[u8]>>, String>;

#[derive(Default)]
pub(crate) struct Snapshot {
    files: Mutex<BTreeMap<PathBuf, Contents>>,
}

impl Clone for Snapshot {
    fn clone(&self) -> Self {
        Self {
            files: Mutex::new(self.files.lock().expect("snapshot lock").clone()),
        }
    }
}

impl Snapshot {
    /// Observe each physical file once, including missing and unreadable files.
    pub(crate) fn read(&self, path: &Path) -> Contents {
        let key = crate::input::path_identity(path).map_err(|error| error.to_string())?;
        self.files
            .lock()
            .expect("snapshot lock")
            .entry(key.clone())
            .or_insert_with(|| read(&key))
            .clone()
    }

    /// Apply and retain every virtual file, including unsupported source languages.
    pub(crate) fn apply_patch(
        &self,
        source: &[u8],
        patch: &crate::input::FilePatch<'_>,
        old: Option<&Path>,
        new: Option<&Path>,
    ) -> Result<crate::input::AppliedPatch> {
        let applied = crate::input::apply_patch(source, patch)?;
        self.stage(old, new, &applied.source)?;
        Ok(applied)
    }

    fn stage(&self, old: Option<&Path>, new: Option<&Path>, source: &[u8]) -> Result<()> {
        if let Some(path) = old {
            self.put(path, None)?;
        }
        if let Some(path) = new {
            self.put(path, Some(Arc::from(source)))?;
        }
        Ok(())
    }

    fn put(&self, path: &Path, contents: Option<Arc<[u8]>>) -> Result<()> {
        let key = crate::input::path_identity(path)?;
        self.files
            .lock()
            .expect("snapshot lock")
            .insert(key, Ok(contents));
        Ok(())
    }
}

fn read(path: &Path) -> Contents {
    match std::fs::read(path) {
        Ok(bytes) => Ok(Some(bytes.into())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.to_string()),
    }
}

#[cfg(test)]
#[path = "../tests/unit/snapshot.rs"]
mod tests;
