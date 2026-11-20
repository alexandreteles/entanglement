use super::{
    build,
    conditional::{Active, Environment, Macro},
    directives::{self, Directive},
    flags::Settings,
};
use crate::analysis::ParsedFile;
use crate::snapshot::Snapshot;
use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;

mod includes;
pub(crate) mod projection;
mod traversal;
pub(crate) mod view;

#[derive(Default)]
pub(crate) struct Project {
    pub units: Vec<Unit>,
}

pub(crate) struct Unit {
    pub cpp: bool,
    pub fallback: bool,
    pub files: BTreeMap<PathBuf, FileView>,
}

#[derive(Default)]
pub(crate) struct FileView {
    pub regions: Vec<Region>,
    pub uses: BTreeMap<usize, Replacement>,
    pub includes: BTreeMap<usize, Include>,
    pub semantic: Option<Arc<crate::model::SemanticStore>>,
}

pub(crate) struct Region {
    pub range: Range<usize>,
    pub active: Active,
    pub order: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Replacement {
    pub range: Range<usize>,
    pub text: Option<String>,
}

pub(crate) struct Include {
    pub range: Range<usize>,
    pub spelling: String,
    pub target: Option<PathBuf>,
    pub active: Active,
    pub system: bool,
}

impl FileView {
    pub fn position(&self, byte: usize) -> Option<(Active, usize, usize)> {
        self.regions
            .iter()
            .filter(|region| region.range.contains(&byte) && region.active != Active::No)
            .min_by_key(|region| (region.active != Active::Yes, region.order))
            .map(|region| (region.active, region.order, byte))
    }
}

struct Source {
    directives: Vec<Directive>,
    guard: Option<String>,
    length: usize,
    words: Vec<(Range<usize>, String)>,
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
        let words = super::lex::tokens(bytes)
            .filter(|token| token.kind == super::lex::Kind::Word)
            .map(|token| {
                (
                    token.range.clone(),
                    String::from_utf8_lossy(&bytes[token.range]).into_owned(),
                )
            })
            .collect();
        Self {
            directives,
            guard,
            length: bytes.len(),
            words,
        }
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
        for path in paths.iter().filter(|path| header(path)) {
            if !units.iter().any(|unit| unit.files.contains_key(path)) {
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
            .filter(|unit| unit.files.contains_key(path) && !unit.fallback)
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
