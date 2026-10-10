//! Read source paths and apply unified diffs in memory.
//!
//! This module does not select a programming language. It returns paths,
//! source bytes, and Tree-sitter edits to the analysis layer.

use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use ignore::WalkBuilder;
use tree_sitter::InputEdit;

/// A file change from a unified diff.
///
/// Paths are relative to the repository root. `old_path` is `None` for a new
/// file. `new_path` is `None` for a deleted file. A rename has both paths.
/// Hunk data stays private; use [`apply_patch`] to apply it.
pub struct FilePatch<'a> {
    /// The safe path of the file before the patch, if the file exists.
    pub old_path: Option<PathBuf>,
    /// The safe path of the file after the patch, if the file exists.
    pub new_path: Option<PathBuf>,
    hunks: Vec<diffy::Patch<'a, [u8]>>,
}

/// Source bytes and ordered edits for one patched file.
///
/// Apply `edits` to an old Tree-sitter tree in this order, then parse
/// `source` with that edited tree. The source is held in memory and is not
/// written to disk.
#[derive(Clone, Debug)]
pub struct AppliedPatch {
    /// The file bytes after the patch.
    pub source: Vec<u8>,
    /// Tree-sitter edits in the order used to produce `source`.
    pub edits: Vec<InputEdit>,
}

/// Stream regular files below a repository directory using Git ignore rules.
///
/// Hidden files are included unless ignored. The iterator skips `.git`, follows
/// symbolic links, and returns traversal errors to the caller.
pub fn discover(
    path: &Path,
) -> crate::Result<impl Iterator<Item = crate::Result<PathBuf>> + Send + use<>> {
    if !path.is_dir() {
        return Err(invalid(format!(
            "repository path is not a directory: {}",
            path.display()
        )));
    }
    let walk = WalkBuilder::new(path)
        .hidden(false)
        .follow_links(true)
        .require_git(false)
        .git_ignore(true)
        .ignore(false)
        .filter_entry(|entry| entry.file_name() != ".git")
        .build();
    Ok(walk.filter_map(|entry| match entry {
        Ok(entry) if entry.file_type().is_some_and(|kind| kind.is_file()) => {
            Some(Ok(entry.into_path()))
        }
        Ok(_) => None,
        Err(error) => Some(Err(error.into())),
    }))
}

/// Return the resolved identity of a path, including a path that does not exist.
///
/// Existing path components are resolved with the filesystem. If the final
/// path does not exist, resolve its nearest existing ancestor and append the
/// remaining path components. Return an I/O error for a broken symbolic link
/// or another path resolution error.
pub(crate) fn path_identity(path: &Path) -> crate::Result<PathBuf> {
    let absolute = std::path::absolute(path)?;
    let mut ancestor = absolute.as_path();
    loop {
        match fs::canonicalize(ancestor) {
            Ok(mut resolved) => {
                resolved.extend(absolute.strip_prefix(ancestor)?.components());
                return Ok(resolved);
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                if fs::symlink_metadata(ancestor)
                    .is_ok_and(|metadata| metadata.file_type().is_symlink())
                {
                    return Err(error.into());
                }
                ancestor = ancestor.parent().ok_or(error)?;
            }
            Err(error) => return Err(error.into()),
        }
    }
}

/// Read a unified diff from a file or standard input.
///
/// Use `-` as `path` to read standard input. Diff text must be valid UTF-8.
/// This function returns an I/O error if a file cannot be read and an invalid
/// data error if the input is not valid UTF-8.
pub fn read_diff(path: &Path) -> crate::Result<String> {
    if path == Path::new("-") {
        let mut diff = String::new();
        io::stdin().read_to_string(&mut diff)?;
        return Ok(diff);
    }

    Ok(fs::read_to_string(path)?)
}

mod apply;
mod parse;

pub(crate) use apply::apply_patch;
pub(crate) use parse::parse_diff;

fn invalid(message: impl Into<String>) -> crate::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into()).into()
}
