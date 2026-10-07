//! Inventory metrics include all source files; conditional members are not proven targets.
use std::path::Path;

use crate::model::FileFacts;

// Go's filename constraints (go/build/syslist.go); no host platform is selected.
const PLATFORMS: &[&str] = &[
    "aix",
    "android",
    "darwin",
    "dragonfly",
    "freebsd",
    "hurd",
    "illumos",
    "ios",
    "js",
    "linux",
    "nacl",
    "netbsd",
    "openbsd",
    "plan9",
    "solaris",
    "wasip1",
    "windows",
    "zos",
    "386",
    "amd64",
    "amd64p32",
    "arm",
    "armbe",
    "arm64",
    "arm64be",
    "loong64",
    "mips",
    "mipsle",
    "mips64",
    "mips64le",
    "mips64p32",
    "mips64p32le",
    "ppc",
    "ppc64",
    "ppc64le",
    "riscv",
    "riscv64",
    "s390",
    "s390x",
    "sparc",
    "sparc64",
    "wasm",
];

pub(super) fn conditional(path: &Path, fact: &FileFacts) -> bool {
    constrained_name(path)
        || has_build_directive(fact)
        || fact
            .imports
            .iter()
            .any(|import| import.source.as_deref() == Some("C"))
}

fn constrained_name(path: &Path) -> bool {
    let Some(stem) = path.file_stem().and_then(|name| name.to_str()) else {
        return true;
    };
    if stem.starts_with(['.', '_']) {
        return true;
    }
    let stem = stem.strip_suffix("_test").unwrap_or(stem);
    stem.rsplit_once('_')
        .is_some_and(|(_, suffix)| PLATFORMS.contains(&suffix))
}

fn has_build_directive(fact: &FileFacts) -> bool {
    let package_start = fact
        .definitions
        .iter()
        .filter(|item| item.kind == crate::model::DefinitionKind::Module)
        .map(|item| item.start_byte)
        .min()
        .unwrap_or(fact.source.len());
    String::from_utf8_lossy(&fact.source[..package_start])
        .lines()
        .map(str::trim_start)
        .any(|line| line.starts_with("//go:build") || line.starts_with("// +build"))
}
