use std::path::{Path, PathBuf};

use crate::languages::go::literals::unquote;
use crate::snapshot::Snapshot;

#[derive(Clone, Eq, PartialEq)]
pub(super) struct Module {
    pub root: PathBuf,
    pub path: String,
}

pub(super) type ModuleState = Result<Option<Module>, ()>;

/// Stop at the nearest manifest even when it is malformed or unreadable.
pub(super) fn nearest(directory: &Path, snapshot: &Snapshot) -> ModuleState {
    for root in directory.ancestors() {
        if let Some(source) = snapshot.read(&root.join("go.mod")).map_err(|_| ())? {
            let path = module_path(std::str::from_utf8(&source).map_err(|_| ())?).ok_or(())?;
            return Ok(Some(Module {
                root: root.to_path_buf(),
                path,
            }));
        }
    }
    Ok(None)
}

fn module_path(source: &str) -> Option<String> {
    let mut directives = source.lines().map(str::trim).filter_map(|line| {
        line.strip_prefix("module")
            .filter(|rest| rest.starts_with(char::is_whitespace))
    });
    let path = directive_value(directives.next()?.trim())?;
    (directives.next().is_none() && valid_path(&path)).then_some(path)
}

fn directive_value(value: &str) -> Option<String> {
    if value.starts_with(['"', '`']) {
        let end = quoted_end(value)?;
        let tail = value[end..].trim();
        return (tail.is_empty() || tail.starts_with("//"))
            .then(|| unquote(&value[..end]))
            .flatten();
    }
    let path = value.split("//").next()?.trim();
    (!path.contains(char::is_whitespace)).then(|| path.to_owned())
}

fn quoted_end(value: &str) -> Option<usize> {
    let quote = value.as_bytes()[0];
    let mut escaped = false;
    for (index, byte) in value.bytes().enumerate().skip(1) {
        if !escaped && byte == quote {
            return Some(index + 1);
        }
        escaped = !escaped && quote == b'"' && byte == b'\\';
    }
    None
}

pub(super) fn valid_path(path: &str) -> bool {
    !path.is_empty()
        && !path
            .chars()
            .any(|ch| ch.is_whitespace() || ch.is_control() || matches!(ch, '\\' | '"' | '`'))
        && path.split('/').all(|part| !matches!(part, "" | "." | ".."))
}

pub(super) fn relative(module: &Module, source: &str) -> Option<PathBuf> {
    let relative = match source.strip_prefix(&module.path)? {
        "" => "",
        rest => rest.strip_prefix('/')?,
    };
    Some(module.root.join(relative))
}
