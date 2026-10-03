//! Read source paths and apply unified diffs in memory.
//!
//! This module does not select a programming language. It returns paths,
//! source bytes, and Tree-sitter edits to the analysis layer.

use std::fs;
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};

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
    if text
        .lines()
        .any(|line| line.starts_with("GIT binary patch") || line.starts_with("Binary files "))
    {
        return Err(invalid("binary patches are not supported"));
    }

    let mut patches = Vec::new();
    let mut remaining = text.as_bytes();
    while !remaining.is_empty() {
        let (line, rest) = take_line(remaining);
        if line.starts_with(b"--- ") {
            let (modified, rest) = take_line(rest);
            if !modified.starts_with(b"+++ ") {
                return Err(invalid("unified diff has an invalid file header"));
            }
            let header_len = remaining.len() - rest.len();
            let header = diffy::Patch::from_bytes(&remaining[..header_len])
                .map_err(|_| invalid("unified diff has an invalid file header"))?;
            let (Some(old), Some(new)) = (header.original(), header.modified()) else {
                return Err(invalid("unified diff has an invalid file header"));
            };
            let old_path = parse_file_path(old, b"a/")?;
            let new_path = parse_file_path(new, b"b/")?;
            if old_path.is_none() && new_path.is_none() {
                return Err(invalid("a patch cannot create and delete a file at once"));
            }
            patches.push(FilePatch {
                old_path,
                new_path,
                hunks: Vec::new(),
            });
            remaining = rest;
        } else if line.starts_with(b"+++ ") {
            return Err(invalid("unified diff has an invalid file header"));
        } else if line.starts_with(b"@@ ") {
            if patches.is_empty() {
                return Err(invalid("unified diff has an invalid hunk header"));
            }
            // Parse one hunk at a time: diffy compares raw coordinates, but a
            // zero-count range is positioned by its normalized endpoint.
            let hunk =
                parse_hunk(remaining).ok_or_else(|| invalid("unified diff has an invalid hunk"))?;
            let line_count = hunk.hunks()[0].lines().len();
            let mut consumed = line.len();
            // Consume validated payload records so header-like source lines
            // cannot be mistaken for the next file's headers.
            for _ in 0..line_count {
                let (line, _) = take_line(&remaining[consumed..]);
                if line.is_empty() {
                    return Err(invalid("unified diff has an invalid hunk"));
                }
                consumed += line.len();
                if remaining[consumed..].starts_with(b"\\ No newline at end of file") {
                    let (marker, _) = take_line(&remaining[consumed..]);
                    consumed += marker.len();
                }
            }
            patches
                .last_mut()
                .ok_or_else(|| invalid("unified diff has an invalid hunk header"))?
                .hunks
                .push(hunk);
            remaining = &remaining[consumed..];
        } else {
            remaining = rest;
        }
    }
    if patches.is_empty() {
        return Err(invalid("unified diff contains no file patches"));
    }
    Ok(patches)
}

fn take_line(input: &[u8]) -> (&[u8], &[u8]) {
    match memchr::memchr(b'\n', input) {
        Some(end) => input.split_at(end + 1),
        None => (input, &[]),
    }
}

fn parse_hunk(input: &[u8]) -> Option<diffy::Patch<'_, [u8]>> {
    let end = memchr::memmem::find(input, b"\n@@ ").map(|index| index + 1);
    let parse_one = |section| {
        let patch = diffy::Patch::from_bytes(section).ok()?;
        (patch.hunks().len() == 1).then_some(patch)
    };
    let mut section = &input[..end.unwrap_or(input.len())];
    loop {
        if let Some(patch) = parse_one(section) {
            return Some(patch);
        }
        // A file header can look like another deletion after a no-newline line.
        let header = memchr::memmem::rfind(section, b"\n--- ")? + 1;
        section = &section[..header];
    }
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
    let mut edits = Vec::with_capacity(patch.hunks.len());
    let mut line_delta = 0_isize;
    let mut previous_old_end = 0_usize;

    for hunk in patch.hunks.iter().flat_map(|patch| patch.hunks()) {
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
    fn applies_adjacent_hunks_with_zero_length_ranges_across_files() {
        let patch = concat!(
            "--- a/replace.rs\n+++ b/replace.rs\n",
            "@@ -1 +1 @@\n-old\n+new\n",
            "@@ -1,0 +2 @@\n+extra\n",
            "--- a/insert.rs\n+++ b/insert.rs\n",
            "@@ -1,0 +2 @@\n+added\n",
            "@@ -2 +2,0 @@\n-remove\n",
            "--- a/header-like.rs\n+++ b/header-like.rs\n",
            "@@ -1,2 +1,2 @@\n--- old\n+++ new\n ++ plus\n",
            "--- a/no-newline.rs\n+++ b/no-newline.rs\n",
            "@@ -1 +1 @@\n-old\n\\ No newline at end of file\n",
            "+new\n\\ No newline at end of file\n",
            "--- a/header-only.rs\n+++ b/header-only.rs\n",
            "--- a/another-header-only.rs\n+++ b/another-header-only.rs\n",
        );
        let patches = parse_diff(patch).unwrap();
        assert_eq!(patches.len(), 6);
        assert_eq!(
            apply_patch(b"old\ntail\n", &patches[0]).unwrap().source,
            b"new\nextra\ntail\n"
        );
        assert_eq!(
            apply_patch(b"head\nremove\ntail\n", &patches[1])
                .unwrap()
                .source,
            b"head\nadded\ntail\n"
        );
        assert_eq!(
            apply_patch(b"-- old\n++ plus\n", &patches[2])
                .unwrap()
                .source,
            b"++ new\n++ plus\n"
        );
        assert_eq!(apply_patch(b"old", &patches[3]).unwrap().source, b"new");
    }

    #[test]
    fn rejects_overlapping_and_out_of_order_hunks() {
        let overlap = concat!(
            "--- f.rs\n+++ f.rs\n@@ -1,2 +1,2 @@\n a\n b\n",
            "@@ -2 +2 @@\n-b\n+B\n",
        );
        let order = concat!(
            "--- f.rs\n+++ f.rs\n@@ -2 +2 @@\n-b\n+B\n",
            "@@ -1 +1 @@\n-a\n+A\n",
        );
        for text in [overlap, order] {
            let patch = parse_diff(text).unwrap();
            assert!(apply_patch(b"a\nb\n", &patch[0]).is_err());
        }
    }
}
