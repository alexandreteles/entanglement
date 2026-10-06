//! Resolve Go names across the files of each package.
//!
//! A package is the set of files in one directory with the same package
//! clause. Import paths resolve through the module path in the nearest
//! `go.mod`. Method dispatch, field access, and dot imports stay unresolved.

use std::collections::BTreeMap;
use std::path::PathBuf;

use rayon::prelude::*;

use crate::languages::ResolutionFamily;
use crate::model::{DefinitionKind, FileFacts, Reference, ReferenceAnalysis, Resolution, SymbolId};

use super::files::indexing::normalize;

mod imports;

use imports::{ImportTarget, Module};

/// The predeclared identifiers of the Go universe block.
const UNIVERSE: &[&str] = &[
    "any",
    "bool",
    "byte",
    "comparable",
    "complex64",
    "complex128",
    "error",
    "float32",
    "float64",
    "int",
    "int8",
    "int16",
    "int32",
    "int64",
    "rune",
    "string",
    "uint",
    "uint8",
    "uint16",
    "uint32",
    "uint64",
    "uintptr",
    "true",
    "false",
    "iota",
    "nil",
    "append",
    "cap",
    "clear",
    "close",
    "complex",
    "copy",
    "delete",
    "imag",
    "len",
    "make",
    "max",
    "min",
    "new",
    "panic",
    "print",
    "println",
    "real",
    "recover",
];

/// Package members by name, each with its symbol and whether it is exported.
type Members = BTreeMap<String, Vec<(SymbolId, bool)>>;

pub(super) fn resolve(facts: &[FileFacts]) -> Vec<Option<Vec<ReferenceAnalysis>>> {
    let index = Index::new(facts);
    facts
        .par_iter()
        .map(|fact| {
            is_go(fact).then(|| {
                fact.references
                    .iter()
                    .map(|reference| ReferenceAnalysis {
                        path: reference.path.clone(),
                        start_byte: reference.start_byte,
                        end_byte: reference.end_byte,
                        resolution: index.resolve_reference(fact, reference),
                    })
                    .collect()
            })
        })
        .collect()
}

struct Index {
    packages: BTreeMap<PathBuf, BTreeMap<String, Members>>,
    modules: BTreeMap<PathBuf, Option<Module>>,
}

impl Index {
    fn new(facts: &[FileFacts]) -> Self {
        let mut packages = BTreeMap::<PathBuf, BTreeMap<String, Members>>::new();
        for fact in facts.iter().filter(|fact| is_go(fact)) {
            let Some(package) = package_name(fact) else {
                continue;
            };
            for directory in directories(fact) {
                let members = packages
                    .entry(directory)
                    .or_default()
                    .entry(package.to_owned())
                    .or_default();
                for (name, member) in members_of(fact) {
                    members.entry(name).or_default().push(member);
                }
            }
        }
        let modules = packages
            .keys()
            .map(|directory| (directory.clone(), imports::nearest_module(directory)))
            .collect();
        Self { packages, modules }
    }

    fn resolve_reference(&self, fact: &FileFacts, reference: &Reference) -> Resolution {
        let name = &reference.path[0];
        if is_local(fact, name, reference.start_byte) {
            return Resolution::Unresolved;
        }
        let own = self.own_members(fact, name);
        if !own.is_empty() {
            return match reference.path.len() {
                1 => resolution(own),
                _ => Resolution::Unresolved,
            };
        }
        let mut bound = self.bound_imports(fact, name);
        match (bound.next(), bound.next(), reference.path.as_slice()) {
            (Some(ImportTarget::Packages(packages)), None, [_, member]) => {
                resolution(self.members(&packages, member, true))
            }
            (Some(ImportTarget::External), None, [_, _]) => Resolution::External,
            (None, _, [_]) if UNIVERSE.contains(&name.as_str()) => Resolution::External,
            _ => Resolution::Unresolved,
        }
    }

    fn own_members(&self, fact: &FileFacts, name: &str) -> Vec<SymbolId> {
        let Some(package) = package_name(fact) else {
            return Vec::new();
        };
        let packages = directories(fact)
            .map(|directory| (directory, package.to_owned()))
            .collect::<Vec<_>>();
        self.members(&packages, name, false)
    }

    fn members(
        &self,
        packages: &[(PathBuf, String)],
        name: &str,
        exported_only: bool,
    ) -> Vec<SymbolId> {
        packages
            .iter()
            .filter_map(|(directory, package)| {
                self.packages.get(directory)?.get(package)?.get(name)
            })
            .flatten()
            .filter(|(_, exported)| *exported || !exported_only)
            .map(|(id, _)| id.clone())
            .collect()
    }
}

fn is_go(fact: &FileFacts) -> bool {
    fact.resolution_family == ResolutionFamily::GoPackages
}

fn is_local(fact: &FileFacts, name: &str, position: usize) -> bool {
    fact.locals.iter().any(|local| {
        local.name == name && local.scope_start <= position && position < local.scope_end
    })
}

fn package_name(fact: &FileFacts) -> Option<&str> {
    fact.definitions
        .iter()
        .find(|definition| definition.kind == DefinitionKind::Module)
        .map(|definition| definition.name.as_str())
}

fn members_of(fact: &FileFacts) -> impl Iterator<Item = (String, (SymbolId, bool))> + '_ {
    fact.definitions
        .iter()
        .filter(|definition| {
            !matches!(
                definition.kind,
                DefinitionKind::Method | DefinitionKind::Module
            )
        })
        .map(|definition| {
            let id = SymbolId {
                file: fact.analysis.path.clone(),
                module: definition.module.clone(),
                name: definition.name.clone(),
                kind: definition.kind.clone(),
                start_byte: definition.start_byte,
            };
            (definition.name.clone(), (id, definition.is_public))
        })
}

fn directories(fact: &FileFacts) -> impl Iterator<Item = PathBuf> + '_ {
    fact.aliases
        .iter()
        .filter_map(|alias| alias.parent().map(normalize))
}

fn resolution(mut symbols: Vec<SymbolId>) -> Resolution {
    symbols.sort();
    symbols.dedup();
    match symbols.len() {
        0 => Resolution::Unresolved,
        1 => Resolution::Exact(symbols.remove(0)),
        _ => Resolution::Ambiguous(symbols),
    }
}
