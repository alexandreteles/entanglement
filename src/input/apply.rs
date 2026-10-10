use tree_sitter::{InputEdit, Point};

use super::{AppliedPatch, FilePatch, invalid};

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
