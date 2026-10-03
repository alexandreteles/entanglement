//! Read source paths and apply unified diffs in memory.
//!
//! This module does not select a programming language. It returns paths,
//! source bytes, and Tree-sitter edits to the analysis layer.

use std::fs;
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};

use diffy::patch_set::{ParseOptions, PatchKind, PatchSet};
use ignore::WalkBuilder;
use tree_sitter::{InputEdit, Point};

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
    patch: diffy::Patch<'a, [u8]>,
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
/// Hidden files are included unless ignored. The iterator skips `.git`, does
/// not follow symbolic links, and returns traversal errors to the caller.
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
        .follow_links(false)
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

/// Parse unified diff text into safe, per-file patches.
///
/// The parser accepts ordinary unified diff headers and Git unified diffs.
/// It supports multiple files, creations, deletions, grouped hunks, and the
/// standard no-newline marker. It rejects unsafe paths, malformed hunk counts,
/// and binary patches. The function does not read or change source files.
pub fn parse_diff(text: &str) -> crate::Result<Vec<FilePatch<'_>>> {
    let hunk_headers = text.lines().filter(|line| line.starts_with("@@ ")).count();
    if text
        .lines()
        .any(|line| line.starts_with("GIT binary patch") || line.starts_with("Binary files "))
    {
        return Err(invalid("binary patches are not supported"));
    }

    let mut hunk_count = 0;
    let patches = PatchSet::parse_bytes(text.as_bytes(), ParseOptions::unidiff())
        .map(|file| {
            let PatchKind::Text(patch) = file?.into_patch() else {
                return Err(invalid("binary patches are not supported"));
            };
            let (Some(old), Some(new)) = (patch.original(), patch.modified()) else {
                return Err(invalid("unified diff has an invalid file header"));
            };
            let old_path = parse_file_path(old, b"a/")?;
            let new_path = parse_file_path(new, b"b/")?;
            if old_path.is_none() && new_path.is_none() {
                return Err(invalid("a patch cannot create and delete a file at once"));
            }
            hunk_count += patch.hunks().len();
            Ok(FilePatch {
                old_path,
                new_path,
                patch,
            })
        })
        .collect::<crate::Result<Vec<_>>>()?;
    if hunk_count != hunk_headers {
        return Err(invalid("unified diff has an invalid hunk header"));
    }
    Ok(patches)
}

/// Apply one file patch to source bytes and return Tree-sitter edits.
///
/// `source` must contain the file named by `patch.old_path`. Pass an empty
/// slice for a new file. The function checks each hunk against the current
/// source, then applies hunks in order. It returns an error if a hunk does not
/// match, if the hunk positions are invalid, or if an edit range is invalid.
/// It does not write to disk.
pub fn apply_patch(source: &[u8], patch: &FilePatch<'_>) -> crate::Result<AppliedPatch> {
    let mut result = source.to_vec();
    let mut edits = Vec::with_capacity(patch.patch.hunks().len());
    let mut line_delta = 0_isize;
    let mut previous_old_end = 0_usize;

    for hunk in patch.patch.hunks() {
        let old = hunk.old_range();
        let new = hunk.new_range();
        let old_line = hunk_line_index(old.start(), old.len())?;
        let new_line = hunk_line_index(new.start(), new.len())?;
        if old_line < previous_old_end {
            return Err(invalid("unified diff hunks overlap or are out of order"));
        }
        let expected_new_line = old_line
            .checked_add_signed(line_delta)
            .ok_or_else(|| invalid("unified diff hunk position is out of range"))?;
        if new_line != expected_new_line {
            return Err(invalid("unified diff hunk positions do not match"));
        }

        let start_byte = line_start(&result, new_line)?;
        let old_bytes = hunk_bytes(hunk.lines(), true);
        let end_byte = start_byte
            .checked_add(old_bytes.len())
            .ok_or_else(|| invalid("unified diff byte range is out of range"))?;
        if result.get(start_byte..end_byte) != Some(old_bytes.as_slice()) {
            return Err(invalid(
                "unified diff context does not match the source file",
            ));
        }
        apply_hunk(&mut result, start_byte, hunk.lines(), &mut edits)?;

        let changed_lines = isize::try_from(new.len())
            .and_then(|new_count| isize::try_from(old.len()).map(|old_count| new_count - old_count))
            .map_err(|_| invalid("unified diff line count is out of range"))?;
        line_delta = line_delta
            .checked_add(changed_lines)
            .ok_or_else(|| invalid("unified diff line count is out of range"))?;
        previous_old_end = old_line
            .checked_add(old.len())
            .ok_or_else(|| invalid("unified diff hunk position is out of range"))?;
    }

    Ok(AppliedPatch {
        source: result,
        edits,
    })
}

fn parse_file_path(raw: &[u8], prefix: &[u8]) -> crate::Result<Option<PathBuf>> {
    if raw == b"/dev/null" {
        return Ok(None);
    }
    let path = raw.strip_prefix(prefix).unwrap_or(raw);
    let path = std::str::from_utf8(path).map_err(|_| invalid("diff path is not valid UTF-8"))?;
    if path.is_empty() || path.contains('\0') || path.contains('\\') {
        return Err(invalid("diff contains an unsafe file path"));
    }
    if path
        .split('/')
        .next()
        .is_some_and(|part| part.ends_with(':'))
    {
        return Err(invalid("diff contains an absolute or drive-prefixed path"));
    }
    let candidate = Path::new(path);
    let mut safe_path = PathBuf::new();
    for component in candidate.components() {
        match component {
            Component::Normal(part) => safe_path.push(part),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(invalid("diff contains a path outside the repository"));
            }
        }
    }
    if safe_path.as_os_str().is_empty() {
        return Err(invalid("diff contains an empty file path"));
    }
    Ok(Some(safe_path))
}

fn hunk_bytes(lines: &[diffy::Line<'_, [u8]>], old: bool) -> Vec<u8> {
    lines
        .iter()
        .filter_map(|line| match (old, line) {
            (true, diffy::Line::Context(bytes) | diffy::Line::Delete(bytes)) => Some(*bytes),
            (false, diffy::Line::Context(bytes) | diffy::Line::Insert(bytes)) => Some(*bytes),
            _ => None,
        })
        .flatten()
        .copied()
        .collect()
}

fn apply_hunk(
    source: &mut Vec<u8>,
    start_byte: usize,
    lines: &[diffy::Line<'_, [u8]>],
    edits: &mut Vec<InputEdit>,
) -> crate::Result<()> {
    // The caller validated the complete old hunk image before applying it.
    let mut cursor = start_byte;
    let mut index = 0;
    while index < lines.len() {
        if let diffy::Line::Context(bytes) = lines[index] {
            cursor += bytes.len();
            index += 1;
            continue;
        }

        let run_start = index;
        while index < lines.len() && !matches!(lines[index], diffy::Line::Context(_)) {
            index += 1;
        }
        let old_bytes = hunk_bytes(&lines[run_start..index], true);
        let new_bytes = hunk_bytes(&lines[run_start..index], false);
        let end_byte = cursor
            .checked_add(old_bytes.len())
            .ok_or_else(|| invalid("unified diff byte range is out of range"))?;
        let new_end_byte = cursor
            .checked_add(new_bytes.len())
            .ok_or_else(|| invalid("unified diff byte range is out of range"))?;
        if old_bytes != new_bytes {
            let start_position = point_at(source, cursor);
            let old_end_position = point_at(source, end_byte);
            let new_end_position = point_after(start_position, &new_bytes);
            edits.push(InputEdit {
                start_byte: cursor,
                old_end_byte: end_byte,
                new_end_byte,
                start_position,
                old_end_position,
                new_end_position,
            });
            drop(source.splice(cursor..end_byte, new_bytes.iter().copied()));
        }
        cursor = new_end_byte;
    }
    Ok(())
}

fn hunk_line_index(start: usize, count: usize) -> crate::Result<usize> {
    if count == 0 {
        Ok(start)
    } else {
        start
            .checked_sub(1)
            .ok_or_else(|| invalid("unified diff line numbers must start at one"))
    }
}

fn line_start(source: &[u8], line_index: usize) -> crate::Result<usize> {
    if line_index == 0 {
        return Ok(0);
    }
    let mut line_count = 0;
    for index in memchr::memchr_iter(b'\n', source) {
        line_count += 1;
        if line_count == line_index {
            return Ok(index + 1);
        }
    }
    if line_count + usize::from(source.last().is_some_and(|byte| *byte != b'\n')) == line_index {
        Ok(source.len())
    } else {
        Err(invalid(
            "unified diff hunk starts past the end of the source",
        ))
    }
}

fn point_at(source: &[u8], byte: usize) -> Point {
    let prefix = &source[..byte];
    let row = memchr::memchr_iter(b'\n', prefix).count();
    let column =
        memchr::memrchr(b'\n', prefix).map_or(prefix.len(), |newline| prefix.len() - newline - 1);
    Point { row, column }
}

fn point_after(start: Point, inserted: &[u8]) -> Point {
    let end = point_at(inserted, inserted.len());
    Point {
        row: start.row + end.row,
        column: if end.row == 0 {
            start.column + end.column
        } else {
            end.column
        },
    }
}

fn invalid(message: impl Into<String>) -> crate::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into()).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn applies_exact_context_and_no_newline_edits() {
        let patch = parse_diff("--- f.rs\n+++ f.rs\n@@ -1 +1 @@\n-old\n+new\n").unwrap();
        assert_eq!(apply_patch(b"old\n", &patch[0]).unwrap().source, b"new\n");
        assert!(apply_patch(b"other\n", &patch[0]).is_err());

        let patch = parse_diff(
            "--- f.rs\n+++ f.rs\n@@ -1 +1 @@\n-old\n\\ No newline at end of file\n+new\n\\ No newline at end of file\n",
        )
        .unwrap();
        assert_eq!(apply_patch(b"old", &patch[0]).unwrap().source, b"new");
    }

    #[test]
    fn keeps_separate_edit_blocks_and_positions() {
        let text = "--- f.rs\n+++ f.rs\n@@ -1,3 +1,3 @@\n-a\n+AA\n keep\n-b\n+B\n";
        let patch = parse_diff(text).unwrap();
        let applied = apply_patch(b"a\nkeep\nb\n", &patch[0]).unwrap();
        assert_eq!(applied.source, b"AA\nkeep\nB\n");
        assert_eq!(
            applied
                .edits
                .iter()
                .map(|edit| (edit.start_byte, edit.old_end_byte, edit.new_end_byte))
                .collect::<Vec<_>>(),
            [(0, 2, 3), (8, 10, 10)]
        );
        assert_eq!(applied.edits[1].start_position, Point { row: 2, column: 0 });
    }

    #[test]
    fn decodes_quoted_paths_and_rejects_unsafe_or_binary_patches() {
        let patch = parse_diff(
            r#"--- "a/src/caf\303\251.rs"
+++ "b/src/caf\303\251.rs"
@@ -1 +1 @@
-old
+new
"#,
        )
        .unwrap();
        assert_eq!(patch[0].old_path.as_deref(), Some(Path::new("src/café.rs")));
        assert!(parse_diff("--- a/../escape.rs\n+++ b/../escape.rs\n").is_err());
        assert!(parse_diff("GIT binary patch\n").is_err());
    }

    #[test]
    fn rejects_unsupported_adjacent_split_hunks() {
        let patch = "--- a/f.rs\n+++ b/f.rs\n@@ -1 +1 @@\n-old\n+new\n@@ -1,0 +2 @@\n+extra\n";
        assert!(parse_diff(patch).is_err());
    }
}
