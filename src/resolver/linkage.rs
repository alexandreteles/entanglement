//! Resolve C and C++ names through translation units and linkage.
//!
//! A translation unit sees its own file and the project headers it includes,
//! directly or through other headers. Functions and variables with external
//! linkage are also visible from every other unit, as the linker sees them.
//! A definition in the reference's own unit wins over one from another unit.

mod scopes;
mod structure;
mod units;

pub(super) use structure::structural_includes;

use std::collections::{BTreeSet, HashMap};

use rayon::prelude::*;

use super::analysis;
use crate::graph::{FileFactsView, ProgramGraph};
use crate::languages::ResolutionFamily;
use crate::model::{
    DefinitionKind, Reference, ReferenceAnalysis, ReferenceKind, Resolution, SymbolId,
};

pub(super) fn resolve(
    facts: &[FileFactsView<'_>],
    graph: &ProgramGraph<'_>,
) -> Vec<Option<Vec<ReferenceAnalysis>>> {
    let index = Index::new(graph);
    facts
        .par_iter()
        .enumerate()
        .map(|(file, fact)| {
            is_linkage(fact).then(|| {
                fact.references()
                    .map(|reference| {
                        analysis(reference, index.resolve(file, fact, reference), None)
                    })
                    .collect()
            })
        })
        .collect()
}

fn is_linkage(fact: &FileFactsView<'_>) -> bool {
    fact.resolution_family == ResolutionFamily::CLinkage
}

struct Symbol {
    id: SymbolId,
    file: usize,
    module: Vec<String>,
    kind: DefinitionKind,
    /// Visible from every translation unit through the linker.
    linked: bool,
}

struct Index {
    symbols: HashMap<String, Vec<Symbol>>,
    units: Vec<units::Unit>,
}

impl Index {
    fn new(graph: &ProgramGraph<'_>) -> Self {
        let mut symbols = HashMap::<String, Vec<Symbol>>::new();
        for file in (0..graph.file_count()).filter(|file| is_linkage(graph.file(*file))) {
            for symbol in graph.symbols_for_file(file) {
                symbols
                    .entry(symbol.name.clone())
                    .or_default()
                    .push(Symbol {
                        id: symbol.semantic_id.clone(),
                        file,
                        module: symbol.module.0.clone(),
                        kind: symbol.kind.clone(),
                        linked: symbol.is_public
                            && matches!(
                                symbol.kind,
                                DefinitionKind::Function
                                    | DefinitionKind::Method
                                    | DefinitionKind::Static
                            ),
                    });
            }
        }
        Self {
            symbols,
            units: units::build(graph),
        }
    }

    fn resolve(&self, file: usize, fact: &FileFactsView<'_>, reference: &Reference) -> Resolution {
        let Some(name) = reference.path.last() else {
            return Resolution::Unresolved;
        };
        if reference.path.len() == 1 && is_local(fact, reference) {
            return Resolution::Unresolved;
        }
        let unit = &self.units[file];
        // C++ names a C struct tag without its keyword.
        let tags = (reference.kind == ReferenceKind::Type
            && reference.path.len() == 1
            && fact.analysis.language == "cpp")
            .then(|| ["struct", "union", "enum"].map(|tag| format!("{tag} {name}")));
        let names = std::iter::once(name.clone()).chain(tags.into_iter().flatten());
        let found = names
            .into_iter()
            .find_map(|name| self.lookup(unit, fact, reference, &name));
        match found {
            Some(resolution) => resolution,
            None if unit.has_external_includes => Resolution::External,
            None => Resolution::Unresolved,
        }
    }

    fn lookup(
        &self,
        unit: &units::Unit,
        fact: &FileFactsView<'_>,
        reference: &Reference,
        name: &str,
    ) -> Option<Resolution> {
        let symbols = self.symbols.get(name)?;
        scopes::candidates(fact, reference)
            .into_iter()
            .find_map(|group| {
                let matches = symbols
                    .iter()
                    .filter(|symbol| {
                        group.contains(&symbol.module)
                            && accepts(reference.kind, &symbol.kind)
                            && (symbol.linked || unit.files.contains(&symbol.file))
                    })
                    .collect::<Vec<_>>();
                select(&matches, &unit.files)
            })
    }
}

fn is_local(fact: &FileFactsView<'_>, reference: &Reference) -> bool {
    fact.locals().any(|local| {
        local.name == reference.path[0]
            && local.scope_start <= reference.start_byte
            && reference.start_byte <= local.scope_end
    })
}

/// C keeps struct tags apart from ordinary names; a macro can stand for either.
fn accepts(reference: ReferenceKind, definition: &DefinitionKind) -> bool {
    let is_type = matches!(
        definition,
        DefinitionKind::Struct
            | DefinitionKind::Union
            | DefinitionKind::Enum
            | DefinitionKind::TypeAlias
    );
    *definition == DefinitionKind::Macro
        || reference == ReferenceKind::MemberInitializer
        || is_type == (reference == ReferenceKind::Type)
}

fn select(matches: &[&Symbol], unit: &BTreeSet<usize>) -> Option<Resolution> {
    let own = matches
        .iter()
        .filter(|symbol| unit.contains(&symbol.file))
        .collect::<Vec<_>>();
    let chosen = if own.is_empty() {
        matches.iter().collect()
    } else {
        own
    };
    match chosen.as_slice() {
        [] => None,
        [symbol] => Some(Resolution::Exact(symbol.id.clone())),
        symbols => {
            let mut ids = symbols
                .iter()
                .map(|symbol| symbol.id.clone())
                .collect::<Vec<_>>();
            ids.sort();
            ids.dedup();
            Some(Resolution::Ambiguous(ids))
        }
    }
}
