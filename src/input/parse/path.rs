use std::path::{Component, Path, PathBuf};

use super::super::invalid;

pub(super) fn parse_file_path(raw: &[u8], prefix: &[u8]) -> crate::Result<Option<PathBuf>> {
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
