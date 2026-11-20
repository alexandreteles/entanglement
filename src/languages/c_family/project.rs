use super::{
    build,
    conditional::{Active, Environment, Macro},
    directives::{self, Directive},
    flags::Settings,
};
use crate::analysis::ParsedFile;
use crate::snapshot::Snapshot;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

mod includes;
mod traversal;

#[derive(Default)]
pub(crate) struct Project {
    pub units: Vec<Unit>,
}

pub(crate) struct Unit {
    pub cpp: bool,
    pub files: BTreeMap<PathBuf, Active>,
}

struct Source {
    directives: Vec<Directive>,
    guard: Option<String>,
}

impl Source {
    fn new(bytes: &[u8]) -> Self {
        let directives = directives::read(bytes);
        let guard = match directives.as_slice() {
            [first, second, .., last]
                if first.name == "ifndef"
                    && second.name == "define"
                    && last.name == "endif"
                    && super::lex::tokens(&bytes[..first.range.start])
                        .chain(super::lex::tokens(&bytes[last.range.end..]))
                        .all(|token| token.kind == super::lex::Kind::Comment)
                    && first.argument.trim()
                        == second.argument.split_whitespace().next().unwrap_or("") =>
            {
                Some(first.argument.trim().into())
            }
            _ => None,
        };
        Self { directives, guard }
    }
}

struct Inventory<'a> {
    sources: BTreeMap<PathBuf, Arc<Source>>,
    snapshot: &'a Snapshot,
}

impl Project {
    pub fn build(parsed: &[ParsedFile], snapshot: &Snapshot) -> crate::Result<Self> {
        let sources: BTreeMap<_, _> = parsed
            .iter()
            .filter(|file| matches!(file.language.as_str(), "source.c" | "source.cpp"))
            .map(|file| {
                (
                    file.facts.target.clone(),
                    Arc::new(Source::new(&file.facts.source)),
                )
            })
            .collect();
        let paths: Vec<_> = sources.keys().cloned().collect();
        if paths.is_empty() {
            return Ok(Self::default());
        }
        let commands = build::commands(&paths, snapshot)?;
        let mut inventory = Inventory { sources, snapshot };
        let mut units = Vec::new();
        for path in &paths {
            if let Some(entries) = commands.get(path) {
                units.extend(
                    entries
                        .iter()
                        .map(|entry| traversal::unit(path, entry, true, &mut inventory)),
                );
            } else if !header(path) {
                units.push(traversal::unit(
                    path,
                    &Settings::standalone(path),
                    false,
                    &mut inventory,
                ));
            }
        }
        Ok(Self { units })
    }

    pub fn dialect(&self, path: &Path) -> Option<bool> {
        let mut choices = self
            .units
            .iter()
            .filter(|unit| {
                unit.files
                    .get(path)
                    .is_some_and(|state| *state != Active::No)
            })
            .map(|unit| unit.cpp);
        let first = choices.next()?;
        choices.all(|cpp| cpp == first).then_some(first)
    }
}

pub(crate) fn is_source(path: &Path) -> bool {
    header(path)
        || matches!(
            path.extension().and_then(|ext| ext.to_str()),
            Some(
                "c" | "C" | "cpp" | "cxx" | "c++" | "cc" | "cppm" | "cxxm" | "c++m" | "ccm" | "ixx"
            )
        )
}

pub(crate) fn header(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|ext| ext.to_str()),
        Some("h" | "H" | "hh" | "hpp" | "hxx" | "h++" | "inc" | "ipp" | "tpp" | "inl" | "tcc")
    )
}
