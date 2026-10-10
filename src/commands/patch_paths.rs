use std::path::{Path, PathBuf};

use crate::input;

/// The old and new paths and file identities for one patch.
pub(super) struct PatchPaths {
    pub(super) old_path: Option<PathBuf>,
    pub(super) new_path: Option<PathBuf>,
    pub(super) old_target: Option<PathBuf>,
    pub(super) new_target: Option<PathBuf>,
}

impl PatchPaths {
    pub(super) fn new(root: &Path, patch: &input::FilePatch<'_>) -> crate::Result<Self> {
        let old_path = patch.old_path.as_ref().map(|path| root.join(path));
        let new_path = patch.new_path.as_ref().map(|path| root.join(path));
        let old_target = old_path
            .as_ref()
            .map(|path| input::path_identity(path))
            .transpose()?;
        let new_target = new_path
            .as_ref()
            .map(|path| input::path_identity(path))
            .transpose()?;
        Ok(Self {
            old_path,
            new_path,
            old_target,
            new_target,
        })
    }

    pub(super) fn ensure_destination_available(
        &self,
        parsed: &[crate::analysis::ParsedFile],
    ) -> crate::Result<()> {
        let Some(new_path) = &self.new_path else {
            return Ok(());
        };
        if self.old_path.as_ref() == Some(new_path) {
            return Ok(());
        }
        let exists = match std::fs::symlink_metadata(new_path) {
            Ok(_) => true,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
            Err(error) => return Err(error.into()),
        };
        if exists
            || parsed
                .iter()
                .any(|file| Some(&file.facts.target) == self.new_target.as_ref())
        {
            return Err(format!(
                "The patch would overwrite another file: {}",
                new_path.display()
            )
            .into());
        }
        Ok(())
    }

    pub(super) fn old_index(&self, parsed: &[crate::analysis::ParsedFile]) -> Option<usize> {
        self.old_target
            .as_ref()
            .and_then(|target| parsed.iter().position(|file| file.facts.target == *target))
    }

    pub(super) fn source(
        &self,
        parsed: &[crate::analysis::ParsedFile],
        old_index: Option<usize>,
    ) -> crate::Result<std::sync::Arc<[u8]>> {
        match (old_index, &self.old_path) {
            (Some(index), _) => Ok(std::sync::Arc::clone(&parsed[index].facts.source)),
            (None, Some(path)) => Ok(std::fs::read(path)?.into()),
            (None, None) => Ok(Vec::new().into()),
        }
    }

    pub(super) fn analysis_path<'a>(
        &'a self,
        parsed: &'a [crate::analysis::ParsedFile],
        old_index: Option<usize>,
        destination: &'a Path,
    ) -> &'a Path {
        old_index
            .filter(|_| self.old_path == self.new_path)
            .map_or(destination, |index| parsed[index].facts.path.as_path())
    }
}
