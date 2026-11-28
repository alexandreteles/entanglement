use globset::{GlobBuilder, GlobSet, GlobSetBuilder};
use std::path::{Component, Path, PathBuf};

/// Normalize MSBuild separators and relative parent components on every host.
pub(super) fn path(base: &Path, value: &str) -> PathBuf {
    let joined = base.join(value.replace('\\', "/"));
    let mut result = PathBuf::new();
    for component in joined.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                result.pop();
            }
            other => result.push(other.as_os_str()),
        }
    }
    result
}

#[derive(Clone, Debug)]
pub(super) struct CompileRule {
    pub include: bool,
    paths: GlobSet,
    excluded: GlobSet,
}

impl CompileRule {
    pub(super) fn new(base: &Path, paths: &str, excluded: &str, include: bool) -> Option<Self> {
        Some(Self {
            include,
            paths: patterns(base, paths)?,
            excluded: patterns(base, excluded)?,
        })
    }
    pub(super) fn applies(&self, source: &Path) -> bool {
        self.paths.is_match(source) && !self.excluded.is_match(source)
    }
}

pub(super) fn patterns(base: &Path, value: &str) -> Option<GlobSet> {
    let mut builder = GlobSetBuilder::new();
    for part in value.split(';').map(str::trim).filter(|s| !s.is_empty()) {
        if part.contains("@(") || part.contains("%(") {
            return None;
        }
        let name = path(base, part).to_string_lossy().replace('\\', "/");
        builder.add(
            GlobBuilder::new(&name)
                .literal_separator(true)
                .backslash_escape(false)
                .build()
                .ok()?,
        );
    }
    builder.build().ok()
}
