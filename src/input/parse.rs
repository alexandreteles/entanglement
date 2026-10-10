mod path;

use self::path::parse_file_path;

use super::{FilePatch, invalid};

/// Parse unified diff text into safe, per-file patches.
///
/// The parser accepts plain and Git diff headers. It supports multiple files,
/// creations, deletions, grouped hunks, and no-newline markers. It rejects
/// unsafe paths, invalid hunk counts, and binary patches. It does not change
/// source files.
pub fn parse_diff(text: &str) -> crate::Result<Vec<FilePatch<'_>>> {
    reject_binary_patch(text)?;
    let mut patches = Vec::new();
    let mut remaining = text.as_bytes();
    while !remaining.is_empty() {
        let (line, rest) = take_line(remaining);
        if line.starts_with(b"--- ") {
            let (patch, consumed) = parse_file_header(remaining)?;
            patches.push(patch);
            remaining = &remaining[consumed..];
        } else if line.starts_with(b"+++ ") {
            return Err(invalid("unified diff has an invalid file header"));
        } else if line.starts_with(b"@@ ") {
            let consumed = append_hunk(remaining, &mut patches)?;
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

fn reject_binary_patch(text: &str) -> crate::Result<()> {
    if text
        .lines()
        .any(|line| line.starts_with("GIT binary patch") || line.starts_with("Binary files "))
    {
        Err(invalid("binary patches are not supported"))
    } else {
        Ok(())
    }
}

fn parse_file_header<'a>(input: &'a [u8]) -> crate::Result<(FilePatch<'a>, usize)> {
    let (_, rest) = take_line(input);
    let (new_header, rest) = take_line(rest);
    if !new_header.starts_with(b"+++ ") {
        return Err(invalid("unified diff has an invalid file header"));
    }
    let header_len = input.len() - rest.len();
    let header = diffy::Patch::from_bytes(&input[..header_len])
        .map_err(|_| invalid("unified diff has an invalid file header"))?;
    let (Some(old), Some(new)) = (header.original(), header.modified()) else {
        return Err(invalid("unified diff has an invalid file header"));
    };
    let old_path = parse_file_path(old, b"a/")?;
    let new_path = parse_file_path(new, b"b/")?;
    if old_path.is_none() && new_path.is_none() {
        return Err(invalid("a patch cannot create and delete a file at once"));
    }
    Ok((
        FilePatch {
            old_path,
            new_path,
            hunks: Vec::new(),
        },
        header_len,
    ))
}

fn append_hunk<'a>(input: &'a [u8], patches: &mut [FilePatch<'a>]) -> crate::Result<usize> {
    let Some(patch) = patches.last_mut() else {
        return Err(invalid("unified diff has an invalid hunk header"));
    };
    let (line, _) = take_line(input);
    let hunk = parse_hunk(input).ok_or_else(|| invalid("unified diff has an invalid hunk"))?;
    let consumed = consume_hunk_payload(input, line.len(), hunk.hunks()[0].lines().len())?;
    patch.hunks.push(hunk);
    Ok(consumed)
}

fn consume_hunk_payload(
    input: &[u8],
    header_len: usize,
    line_count: usize,
) -> crate::Result<usize> {
    // Consume the payload so header-like source lines stay inside this hunk.
    let mut consumed = header_len;
    for _ in 0..line_count {
        let (line, _) = take_line(&input[consumed..]);
        if line.is_empty() {
            return Err(invalid("unified diff has an invalid hunk"));
        }
        consumed += line.len();
        if input[consumed..].starts_with(b"\\ No newline at end of file") {
            let (marker, _) = take_line(&input[consumed..]);
            consumed += marker.len();
        }
    }
    Ok(consumed)
}

fn take_line(input: &[u8]) -> (&[u8], &[u8]) {
    match memchr::memchr(b'\n', input) {
        Some(end) => input.split_at(end + 1),
        None => (input, &[]),
    }
}

fn parse_hunk(input: &[u8]) -> Option<diffy::Patch<'_, [u8]>> {
    // Diffy compares raw coordinates. A zero-count range uses its endpoint.
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
        // A deletion header can look like a second file header after a marker.
        let header = memchr::memmem::rfind(section, b"\n--- ")? + 1;
        section = &section[..header];
    }
}
