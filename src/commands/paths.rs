use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::input;

pub(super) type SourcePaths = Box<dyn Iterator<Item = crate::Result<PathBuf>> + Send>;

/// Find the diff root and source files for a file or directory target.
///
/// A file target must match one old-file path. Candidate analysis includes
/// the nearest repository, or the diff root when no repository is found.
pub(super) fn patch_sources(
    path: &Path,
    patches: &[input::FilePatch],
    single: bool,
) -> crate::Result<(PathBuf, SourcePaths)> {
    let target = std::path::absolute(path)?;
    if single && !target.is_file() {
        return Err("PATCH requires a source file".into());
    }
    let root = if target.is_file() {
        file_patch_root(&target, patches)?
    } else {
        target.clone()
    };
    validate_patches(&root, patches)?;
    let paths: SourcePaths = if target.is_file() && single {
        Box::new(std::iter::once(Ok(target.clone())))
    } else {
        let repository = if target.is_file() {
            nearest_project_root(&target, &root)?
        } else {
            root.as_path()
        };
        let paths = input::discover(repository)?;
        let old_paths: Vec<_> = patches
            .iter()
            .filter_map(|patch| patch.old_path.as_ref())
            .map(|path| root.join(path))
            .collect();
        Box::new(paths.chain(old_paths.into_iter().map(Ok)))
    };
    Ok((root, paths))
}

fn file_patch_root(target: &Path, patches: &[input::FilePatch]) -> crate::Result<PathBuf> {
    if patches.len() != 1 {
        return Err("A file target requires one file patch".into());
    }
    let old = patches[0]
        .old_path
        .as_ref()
        .ok_or("A file target requires an old-file path")?;
    if !target.ends_with(old) {
        return Err("The diff old path does not match the selected file".into());
    }
    let mut root = target.to_path_buf();
    for _ in old.components() {
        root.pop();
    }
    Ok(root)
}

fn validate_patches(root: &Path, patches: &[input::FilePatch]) -> crate::Result<()> {
    if patches.is_empty() {
        return Err("The diff has no file patches".into());
    }
    let mut touched = BTreeSet::new();
    for patch in patches {
        let targets: BTreeSet<_> = patch
            .old_path
            .iter()
            .chain(&patch.new_path)
            .map(|path| input::path_identity(&root.join(path)))
            .collect::<crate::Result<_>>()?;
        if targets.into_iter().any(|target| !touched.insert(target)) {
            return Err("The diff changes one file target more than once".into());
        }
    }
    Ok(())
}

fn nearest_project_root<'a>(target: &'a Path, fallback: &'a Path) -> crate::Result<&'a Path> {
    let manifests = crate::languages::Registry::shared()?.project_manifests();
    Ok(target
        .ancestors()
        .skip(1)
        .find(|directory| is_project_root(directory, &manifests))
        .unwrap_or(fallback))
}

fn is_project_root(directory: &Path, manifests: &[&str]) -> bool {
    directory.join(".git").exists()
        || manifests
            .iter()
            .any(|manifest| directory.join(manifest).is_file())
}
