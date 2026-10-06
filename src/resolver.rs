//! Resolve Rust names across the analyzed files.
//!
//! The resolver uses Tree-sitter facts from each file. It does not infer types
//! or resolve method dispatch. It leaves uncertain names unresolved.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use rayon::prelude::*;

use crate::model::{FileFacts, ReferenceAnalysis, Resolution, SymbolId};

mod files;
mod indexing;
mod module_graph;
mod packages;
mod resolution;

/// Keep a shared-file fallback separate from every logical crate root.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum CrateKey {
    Logical(PathBuf),
    Detached(PathBuf),
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct ModuleKey {
    crate_root: CrateKey,
    path: Vec<String>,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum Target {
    Symbol(SymbolId),
    Module(ModuleKey),
    External,
    Ambiguous(Vec<SymbolId>),
    Unresolved,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct FileContext {
    crate_root: CrateKey,
    module: Vec<String>,
}

#[derive(Clone, Debug)]
struct Symbol {
    id: SymbolId,
    file: usize,
    scope_start: usize,
    scope_end: usize,
    is_local: bool,
    is_public: bool,
}

#[derive(Clone, Debug)]
struct ImportEntry {
    file: usize,
    module: ModuleKey,
    name: String,
    path: Vec<String>,
    scope_start: usize,
    scope_end: usize,
    is_local: bool,
    is_public: bool,
}

struct Index {
    contexts: Vec<Vec<FileContext>>,
    symbols: BTreeMap<ModuleKey, BTreeMap<String, Vec<Symbol>>>,
    modules: BTreeSet<ModuleKey>,
    imports: Vec<ImportEntry>,
    import_targets: Vec<Vec<Target>>,
}

/// Resolve references in all facts and store their results in each analysis.
///
/// Call this after all files have been parsed. The function sorts symbols and
/// ambiguity results to keep output stable. It leaves method calls and names
/// that cannot be resolved without guessing as `Unresolved`.
pub(crate) fn resolve(facts: &mut [FileFacts], selection: crate::metrics::selection::Selection) {
    let mut index = Index::new(facts);
    index.resolve_imports();
    let file_resolutions = files::resolve(facts);
    let package_resolutions = packages::resolve(facts);
    facts
        .par_iter_mut()
        .enumerate()
        .for_each(|(file_index, fact)| {
            if let Some(resolutions) = file_resolutions[file_index]
                .as_ref()
                .or(package_resolutions[file_index].as_ref())
            {
                fact.analysis.resolution = resolutions.clone();
                return;
            }
            if fact.resolution_family != crate::languages::ResolutionFamily::RustCrates {
                fact.analysis.resolution.clear();
                return;
            }
            let context = &index.contexts[file_index];
            fact.analysis.resolution = fact
                .references
                .iter()
                .map(|reference| ReferenceAnalysis {
                    path: reference.path.clone(),
                    start_byte: reference.start_byte,
                    end_byte: reference.end_byte,
                    resolution: index.to_resolution(
                        context
                            .iter()
                            .map(|context| {
                                match index.resolve_reference(file_index, context, reference, fact)
                                {
                                    Resolution::Exact(symbol) => Target::Symbol(symbol),
                                    Resolution::Ambiguous(symbols) => Target::Ambiguous(symbols),
                                    Resolution::External => Target::External,
                                    Resolution::Unresolved => Target::Unresolved,
                                }
                            })
                            .collect(),
                    ),
                })
                .collect();
        });
    drop(index);
    if selection.includes(crate::metrics::selection::Metric::Cogc) {
        crate::metrics::recursion::annotate(facts);
    }
}

fn joined_path(base: &[String], relative: &[String]) -> Vec<String> {
    base.iter().chain(relative).cloned().collect()
}
