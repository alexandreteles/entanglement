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
#[derive(Clone, Debug)]
pub struct FilePatch {
    /// The safe path of the file before the patch, if the file exists.
    pub old_path: Option<PathBuf>,
    /// The safe path of the file after the patch, if the file exists.
    pub new_path: Option<PathBuf>,
    hunks: Vec<Hunk>,
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

#[derive(Clone, Debug)]
struct Hunk {
    old_start: usize,
    old_count: usize,
    new_start: usize,
    new_count: usize,
    lines: Vec<HunkLine>,
}

#[derive(Clone, Debug)]
struct HunkLine {
    old: Option<Vec<u8>>,
    new: Option<Vec<u8>>,
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
pub fn parse_diff(text: &str) -> crate::Result<Vec<FilePatch>> {
    let lines: Vec<&[u8]> = text
        .as_bytes()
        .split_inclusive(|byte| *byte == b'\n')
        .collect();
    let mut patches = Vec::new();
    let mut index = 0;

    while index < lines.len() {
        let header = control_line(lines[index]);
        if !header.starts_with(b"--- ") {
            if header.starts_with(b"GIT binary patch") || header.starts_with(b"Binary files ") {
                return Err(invalid("binary patches are not supported"));
            }
            index += 1;
            continue;
        }

        let old_path = parse_file_path(&header[4..], b"a/")?;
        index += 1;
        let Some(new_header) = lines.get(index).map(|line| control_line(line)) else {
            return Err(invalid("unified diff has no new-file header"));
        };
        if !new_header.starts_with(b"+++ ") {
            return Err(invalid("unified diff has an invalid new-file header"));
        }
        let new_path = parse_file_path(&new_header[4..], b"b/")?;
        index += 1;

        let mut hunks = Vec::new();
        while index < lines.len() {
            let header = control_line(lines[index]);
            if header.starts_with(b"@@ ") {
                let (hunk, next_index) = parse_hunk(&lines, index)?;
                hunks.push(hunk);
                index = next_index;
            } else if header.starts_with(b"--- ")
                || header.starts_with(b"diff --git ")
                || header.starts_with(b"GIT binary patch")
                || header.starts_with(b"Binary files ")
            {
                break;
            } else {
                index += 1;
            }
        }

        if old_path.is_none() && new_path.is_none() {
            return Err(invalid("a patch cannot create and delete a file at once"));
        }
        patches.push(FilePatch {
            old_path,
            new_path,
            hunks,
        });
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
pub fn apply_patch(source: &[u8], patch: &FilePatch) -> crate::Result<AppliedPatch> {
    let mut result = source.to_vec();
    let mut edits = Vec::with_capacity(patch.hunks.len());
    let mut line_delta = 0_isize;
    let mut previous_old_end = 0_usize;

    for hunk in &patch.hunks {
        let old_line = hunk_line_index(hunk.old_start, hunk.old_count)?;
        let new_line = hunk_line_index(hunk.new_start, hunk.new_count)?;
        if old_line < previous_old_end {
            return Err(invalid("unified diff hunks overlap or are out of order"));
        }
        let expected_new_line = old_line
            .checked_add_signed(line_delta)
            .ok_or_else(|| invalid("unified diff hunk position is out of range"))?;
        if new_line != expected_new_line {
            return Err(invalid("unified diff hunk positions do not match"));
        }

        let current_line = old_line
            .checked_add_signed(line_delta)
            .ok_or_else(|| invalid("unified diff hunk position is out of range"))?;
        let start_byte = line_start(&result, current_line)?;
        let old_bytes = hunk_bytes(&hunk.lines, true);
        let end_byte = start_byte
            .checked_add(old_bytes.len())
            .ok_or_else(|| invalid("unified diff byte range is out of range"))?;
        if result.get(start_byte..end_byte) != Some(old_bytes.as_slice()) {
            return Err(invalid(
                "unified diff context does not match the source file",
            ));
        }
        apply_hunk(&mut result, start_byte, &hunk.lines, &mut edits)?;

        let changed_lines = isize::try_from(hunk.new_count)
            .and_then(|new_count| {
                isize::try_from(hunk.old_count).map(|old_count| new_count - old_count)
            })
            .map_err(|_| invalid("unified diff line count is out of range"))?;
        line_delta = line_delta
            .checked_add(changed_lines)
            .ok_or_else(|| invalid("unified diff line count is out of range"))?;
        previous_old_end = old_line
            .checked_add(hunk.old_count)
            .ok_or_else(|| invalid("unified diff hunk position is out of range"))?;
    }

    Ok(AppliedPatch {
        source: result,
        edits,
    })
}

fn parse_hunk(lines: &[&[u8]], start: usize) -> crate::Result<(Hunk, usize)> {
    let header = control_line(lines[start]);
    let (old_start, old_count, new_start, new_count) = parse_hunk_header(header)?;
    let mut old_seen = 0_usize;
    let mut new_seen = 0_usize;
    let mut hunk_lines: Vec<HunkLine> = Vec::new();
    let mut index = start + 1;

    while old_seen < old_count || new_seen < new_count {
        let Some(line) = lines.get(index) else {
            return Err(invalid("unified diff ends inside a hunk"));
        };
        if line.starts_with(b"\\ No newline at end of file") {
            mark_no_newline(&mut hunk_lines)?;
            index += 1;
            continue;
        }
        let Some((&kind, content)) = line.split_first() else {
            return Err(invalid("unified diff has an empty hunk line"));
        };
        let content = content.to_vec();
        match kind {
            b' ' => {
                old_seen += 1;
                new_seen += 1;
                hunk_lines.push(HunkLine {
                    old: Some(content.clone()),
                    new: Some(content),
                });
            }
            b'+' => {
                new_seen += 1;
                hunk_lines.push(HunkLine {
                    old: None,
                    new: Some(content),
                });
            }
            b'-' => {
                old_seen += 1;
                hunk_lines.push(HunkLine {
                    old: Some(content),
                    new: None,
                });
            }
            _ => return Err(invalid("unified diff has an invalid hunk line")),
        }
        if old_seen > old_count || new_seen > new_count {
            return Err(invalid(
                "unified diff hunk line counts do not match its header",
            ));
        }
        index += 1;
    }

    while lines
        .get(index)
        .is_some_and(|line| control_line(line).starts_with(b"\\ No newline at end of file"))
    {
        mark_no_newline(&mut hunk_lines)?;
        index += 1;
    }

    Ok((
        Hunk {
            old_start,
            old_count,
            new_start,
            new_count,
            lines: hunk_lines,
        },
        index,
    ))
}

fn parse_hunk_header(line: &[u8]) -> crate::Result<(usize, usize, usize, usize)> {
    let Some(rest) = line.strip_prefix(b"@@ -") else {
        return Err(invalid("unified diff has an invalid hunk header"));
    };
    let Some(plus) = rest.iter().position(|byte| *byte == b'+') else {
        return Err(invalid("unified diff has an invalid hunk header"));
    };
    let old = rest
        .get(..plus)
        .and_then(|range| range.strip_suffix(b" "))
        .ok_or_else(|| invalid("unified diff has an invalid hunk header"))?;
    let new = rest
        .get(plus + 1..)
        .and_then(|range| range.split(|byte| *byte == b' ').next())
        .ok_or_else(|| invalid("unified diff has an invalid hunk header"))?;
    let Some(after_new) = rest.get(plus + 1 + new.len()..) else {
        return Err(invalid("unified diff has an invalid hunk header"));
    };
    if !after_new.starts_with(b" @@") {
        return Err(invalid("unified diff has an invalid hunk header"));
    }
    let (old_start, old_count) = parse_range(old)?;
    let (new_start, new_count) = parse_range(new)?;
    if (old_count > 0 && old_start == 0) || (new_count > 0 && new_start == 0) {
        return Err(invalid("unified diff line numbers must start at one"));
    }
    Ok((old_start, old_count, new_start, new_count))
}

fn parse_range(range: &[u8]) -> crate::Result<(usize, usize)> {
    let mut parts = range.split(|byte| *byte == b',');
    let start = parse_usize(parts.next().unwrap_or_default())?;
    let count = match parts.next() {
        Some(count) => parse_usize(count)?,
        None => 1,
    };
    if parts.next().is_some() {
        return Err(invalid("unified diff has an invalid hunk range"));
    }
    Ok((start, count))
}

fn parse_usize(value: &[u8]) -> crate::Result<usize> {
    let text = std::str::from_utf8(value).map_err(|_| invalid("unified diff has a bad number"))?;
    text.parse()
        .map_err(|_| invalid("unified diff has a bad number"))
}

fn parse_file_path(raw: &[u8], prefix: &[u8]) -> crate::Result<Option<PathBuf>> {
    let raw = raw.split(|byte| *byte == b'\t').next().unwrap_or_default();
    let path = if raw.starts_with(b"\"") {
        parse_quoted_path(raw)?
    } else {
        raw.to_vec()
    };
    if path == b"/dev/null" {
        return Ok(None);
    }
    let path = path.strip_prefix(prefix).unwrap_or(&path);
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

fn parse_quoted_path(raw: &[u8]) -> crate::Result<Vec<u8>> {
    let mut result = Vec::new();
    let mut index = 1;
    while let Some(&byte) = raw.get(index) {
        match byte {
            b'"' => {
                if raw[index + 1..]
                    .iter()
                    .any(|byte| !byte.is_ascii_whitespace())
                {
                    return Err(invalid("diff has data after a quoted path"));
                }
                return Ok(result);
            }
            b'\\' => {
                index += 1;
                let Some(&escaped) = raw.get(index) else {
                    return Err(invalid("diff has an unfinished path escape"));
                };
                match escaped {
                    b'"' | b'\\' => result.push(escaped),
                    b't' => result.push(b'\t'),
                    b'n' => result.push(b'\n'),
                    b'r' => result.push(b'\r'),
                    b'0'..=b'7' => {
                        let end = (index + 3).min(raw.len());
                        let digits = &raw[index..end];
                        if digits.iter().any(|digit| !(b'0'..=b'7').contains(digit)) {
                            return Err(invalid("diff has an invalid octal path escape"));
                        }
                        let value = std::str::from_utf8(digits)
                            .ok()
                            .and_then(|digits| u8::from_str_radix(digits, 8).ok())
                            .ok_or_else(|| invalid("diff has an invalid octal path escape"))?;
                        result.push(value);
                        index = end - 1;
                    }
                    _ => return Err(invalid("diff has an invalid path escape")),
                }
            }
            _ => result.push(byte),
        }
        index += 1;
    }
    Err(invalid("diff has an unfinished quoted path"))
}

fn mark_no_newline(lines: &mut [HunkLine]) -> crate::Result<()> {
    let Some(line) = lines.last_mut() else {
        return Err(invalid("no-newline marker has no hunk line"));
    };
    if let Some(old) = &mut line.old {
        remove_line_feed(old);
    }
    if let Some(new) = &mut line.new {
        remove_line_feed(new);
    }
    Ok(())
}

fn remove_line_feed(bytes: &mut Vec<u8>) {
    if bytes.last() == Some(&b'\n') {
        bytes.pop();
    }
}

fn hunk_bytes(lines: &[HunkLine], old: bool) -> Vec<u8> {
    lines
        .iter()
        .filter_map(|line| {
            if old {
                line.old.as_ref()
            } else {
                line.new.as_ref()
            }
        })
        .flatten()
        .copied()
        .collect()
}

fn apply_hunk(
    source: &mut Vec<u8>,
    start_byte: usize,
    lines: &[HunkLine],
    edits: &mut Vec<InputEdit>,
) -> crate::Result<()> {
    let mut cursor = start_byte;
    let mut index = 0;
    while index < lines.len() {
        if lines[index].old.is_some() && lines[index].new.is_some() {
            cursor += lines[index].old.as_ref().map_or(0, Vec::len);
            index += 1;
            continue;
        }

        let run_start = index;
        while index < lines.len() && !(lines[index].old.is_some() && lines[index].new.is_some()) {
            index += 1;
        }
        let old_bytes = hunk_bytes(&lines[run_start..index], true);
        let new_bytes = hunk_bytes(&lines[run_start..index], false);
        let end_byte = cursor
            .checked_add(old_bytes.len())
            .ok_or_else(|| invalid("unified diff byte range is out of range"))?;
        if source.get(cursor..end_byte) != Some(old_bytes.as_slice()) {
            return Err(invalid(
                "unified diff context does not match the source file",
            ));
        }
        if old_bytes != new_bytes {
            let start_position = point_at(source, cursor);
            let old_end_position = point_at(source, end_byte);
            let new_end_position = point_after(start_position, &new_bytes);
            let new_end_byte = cursor
                .checked_add(new_bytes.len())
                .ok_or_else(|| invalid("unified diff byte range is out of range"))?;
            edits.push(InputEdit {
                start_byte: cursor,
                old_end_byte: end_byte,
                new_end_byte,
                start_position,
                old_end_position,
                new_end_position,
            });
            let mut updated = Vec::with_capacity(source.len() - old_bytes.len() + new_bytes.len());
            updated.extend_from_slice(&source[..cursor]);
            updated.extend_from_slice(&new_bytes);
            updated.extend_from_slice(&source[end_byte..]);
            *source = updated;
        }
        cursor = cursor
            .checked_add(new_bytes.len())
            .ok_or_else(|| invalid("unified diff byte range is out of range"))?;
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
    for (index, byte) in source.iter().enumerate() {
        if *byte == b'\n' {
            line_count += 1;
            if line_count == line_index {
                return Ok(index + 1);
            }
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
    let row = prefix.iter().filter(|byte| **byte == b'\n').count();
    let column = prefix
        .iter()
        .rposition(|byte| *byte == b'\n')
        .map_or(prefix.len(), |newline| prefix.len() - newline - 1);
    Point { row, column }
}

fn point_after(start: Point, inserted: &[u8]) -> Point {
    let newlines = inserted.iter().filter(|byte| **byte == b'\n').count();
    if newlines == 0 {
        Point {
            row: start.row,
            column: start.column + inserted.len(),
        }
    } else {
        Point {
            row: start.row + newlines,
            column: inserted
                .iter()
                .rposition(|byte| *byte == b'\n')
                .map_or(start.column + inserted.len(), |newline| {
                    inserted.len() - newline - 1
                }),
        }
    }
}

fn control_line(line: &[u8]) -> &[u8] {
    let line = line.strip_suffix(b"\n").unwrap_or(line);
    line.strip_suffix(b"\r").unwrap_or(line)
}

fn invalid(message: impl Into<String>) -> crate::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into()).into()
}
