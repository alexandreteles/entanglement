use std::sync::Arc;

use super::*;
use crate::graph::BindingType;
use crate::metrics::halstead::HalsteadMetrics;
use crate::model::{DefinitionKind, FileAnalysis, FileFacts, LocalBinding, ModulePath};

fn fixture() -> FileFacts {
    let definition = |context_id, module: &str| Definition {
        name: "shared".into(),
        kind: DefinitionKind::Function,
        module: ModulePath(vec![module.into()]),
        start_byte: 10,
        end_byte: 90,
        scope_start: 0,
        scope_end: 100,
        is_public: true,
        inline_module: false,
        external_module: false,
        context_id,
    };
    let semantic = |context_id, type_name: &str| SemanticFacts {
        context_id,
        bindings: vec![BindingType {
            name: "value".into(),
            name_start_byte: 25,
            name_end_byte: 30,
            scope_start: 20,
            scope_end: 80,
            type_name: Some(type_name.into()),
            alias_target: None,
            callable_reference: false,
            immutable: true,
        }],
        ..SemanticFacts::default()
    };
    FileFacts {
        definitions: vec![
            definition(0, "excluded"),
            definition(1, "included"),
            definition(2, "included"),
        ],
        semantic: vec![semantic(1, "First"), semantic(2, "Second")],
        locals: vec![LocalBinding {
            name: "value".into(),
            start_byte: 25,
            end_byte: 30,
            scope_start: 20,
            scope_end: 80,
            context_id: 1,
        }],
        source: Arc::from(vec![b' '; 100]),
        target: PathBuf::from("fixture.rs"),
        aliases: vec![PathBuf::from("fixture.rs")],
        path: PathBuf::from("fixture.rs"),
        module: ModulePath::default(),
        resolution_family: ResolutionFamily::RustCrates,
        file_module_rules: None,
        imports: Vec::new(),
        exports: Vec::new(),
        references: Vec::new(),
        analysis: FileAnalysis {
            path: "fixture.rs".into(),
            hash: String::new(),
            language: "rust".into(),
            nloc: 0,
            halstead: HalsteadMetrics::calculate(0, 0, 0, 0),
            maintainability_index: None,
            functions: Vec::new(),
            injections: Vec::new(),
            resolution: Vec::new(),
        },
    }
}

#[test]
fn projections_reuse_indexes_and_preserve_canonical_identity_and_context_evidence() {
    let facts = [fixture()];
    let graph = ProgramGraph::new(&facts);
    let project = |contexts| {
        graph.project(vec![FileFactsView::projected(
            &facts[0],
            contexts,
            ResolutionFamily::RustCrates,
            None,
        )])
    };
    let projected = project(HashSet::from([1]));
    assert!(Arc::ptr_eq(&graph.indexes, &projected.indexes));
    assert!(std::ptr::eq(graph.aliases(), projected.aliases()));
    let id = ProgramSymbolId {
        file_index: 0,
        definition_index: 1,
    };
    let symbols = projected.symbols_for_file(0).collect::<Vec<_>>();
    assert_eq!(symbols.len(), 1);
    assert_eq!(symbols[0].id, id);
    assert!(std::ptr::eq(
        projected.definition(id).unwrap(),
        &facts[0].definitions[1]
    ));
    assert!(std::ptr::eq(
        projected.file(0).definitions().next().unwrap(),
        &facts[0].definitions[1]
    ));
    assert!(
        projected
            .symbol(ProgramSymbolId {
                file_index: 0,
                definition_index: 0
            })
            .is_none()
    );
    assert_eq!(
        projected.binding_type(0, "value", 1, 50),
        Some("First".into())
    );
    assert_eq!(projected.binding_type(0, "value", 2, 50), None);
    assert!(projected.is_local_scope(0, 1, 20, 80, 50));
    let nearest = projected.scope_at(0, 1, 50).unwrap();
    assert_eq!(
        projected.scope(nearest).unwrap().parent,
        graph.scope(nearest).unwrap().parent
    );

    // Excluded exact keys must behave as absent, allowing the module
    // qualification fallback used by Rust resolution to find the view's declaration.
    let excluded_target = graph
        .symbol_id(ProgramSymbolId {
            file_index: 0,
            definition_index: 0,
        })
        .unwrap();
    assert_eq!(
        projected.unique_symbol_for_target(excluded_target),
        Some(id)
    );
    let ambiguous = project(HashSet::from([1, 2]));
    assert_eq!(ambiguous.unique_symbol_for_target(excluded_target), None);
    assert_eq!(
        ambiguous.unique_symbol_for_target(graph.symbol_id(id).unwrap()),
        None
    );
}

#[test]
fn projections_preserve_conflicting_binding_evidence_within_an_eligible_context() {
    let mut facts = [fixture()];
    facts[0].semantic[1].context_id = 1;
    let graph = ProgramGraph::new(&facts);
    let projected = graph.project(vec![FileFactsView::projected(
        &facts[0],
        HashSet::from([1]),
        ResolutionFamily::RustCrates,
        None,
    )]);
    assert!(projected.has_local_binding(0, "value", 1, 50));
    assert_eq!(projected.binding_type(0, "value", 1, 50), None);
}
