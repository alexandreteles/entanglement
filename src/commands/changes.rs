use tree_sitter::InputEdit;

use super::patch_paths::PatchPaths;
use crate::{metrics::selection::Selection, model};

/// Paths and edits for one virtual file change.
pub(super) struct PatchChange {
    pub(super) paths: PatchPaths,
    pub(super) edits: Vec<InputEdit>,
}

impl PatchChange {
    /// Compare reports and function changes for this file change.
    pub(super) fn compare(
        self,
        before: &[model::FileFacts],
        after: &[model::FileFacts],
        selection: Selection,
    ) -> model::FilePatchAnalysis {
        let old = self
            .paths
            .old_target
            .as_ref()
            .and_then(|target| before.iter().find(|file| file.target == *target))
            .map(|file| file.analysis.clone());
        let new = self
            .paths
            .new_target
            .as_ref()
            .and_then(|target| after.iter().find(|file| file.target == *target))
            .map(|file| file.analysis.clone());
        model::FilePatchAnalysis {
            path: self
                .paths
                .new_path
                .or(self.paths.old_path)
                .expect("A patch has a path")
                .display()
                .to_string(),
            maintainability_index: crate::report_delta::maintainability_delta(
                old.as_ref()
                    .and_then(|file| file.maintainability_index.as_ref()),
                new.as_ref()
                    .and_then(|file| file.maintainability_index.as_ref()),
            ),
            halstead: crate::report_delta::halstead_delta(
                old.as_ref().map(|file| &file.halstead),
                new.as_ref().map(|file| &file.halstead),
                &self.edits,
                selection,
            ),
            functions: crate::report_delta::function_deltas(
                old.as_ref(),
                new.as_ref(),
                &self.edits,
                selection,
            ),
            before: old,
            after: new,
        }
    }
}
