use super::units;
use crate::graph::ProgramGraph;
use crate::model::{DefinitionKind, ModulePath, Resolution, SymbolId};

/// Resolve each include to the project file it names, as a structural module import.
pub(in crate::resolver) fn structural_includes(
    graph: &ProgramGraph<'_>,
) -> Vec<(usize, usize, usize, String, Resolution)> {
    let includes = units::Includes::new(graph);
    let module = |file: usize, source: &str| SymbolId {
        file: graph.file(file).analysis.path.clone(),
        module: ModulePath::default(),
        name: source.to_owned(),
        kind: DefinitionKind::Module,
        start_byte: 0,
    };
    (0..graph.file_count())
        .flat_map(|file| {
            includes
                .of(graph.file(file))
                .into_iter()
                .map(move |(import, targets)| (file, import, targets))
        })
        .map(|(file, import, targets)| {
            let source = import.source.clone().unwrap_or_default();
            let mut modules = targets.iter().map(|target| module(*target, &source));
            let resolution = match targets.len() {
                0 => Resolution::External,
                1 => Resolution::Exact(modules.next().expect("one include target")),
                _ => Resolution::Ambiguous(modules.collect()),
            };
            (file, import.start_byte, import.end_byte, source, resolution)
        })
        .collect()
}
