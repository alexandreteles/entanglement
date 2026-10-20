use crate::graph::ProgramSymbolId;
use crate::graph::{ReceiverScope, SemanticFacts, program::FileFactsView};
use crate::languages::ResolutionFamily;
use crate::model::BindingEvidence;
use std::sync::Arc;
use std::{collections::HashSet, path::PathBuf};

use super::*;
use crate::metrics::halstead::HalsteadMetrics;
use crate::model::{
    DefinitionKind, FileAnalysis, FileFacts, LocalBinding, ModulePath, SemanticStore,
};

fn fixture() -> FileFacts {
    let definition = |context_id, module: &str| Definition {
        declaration: None,
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
    let semantic = |context_id| SemanticFacts {
        context_id,
        ..SemanticFacts::default()
    };
    let binding = |context_id, type_name: &str, evidence| LocalBinding {
        name: "value".into(),
        start_byte: 25,
        end_byte: 30,
        scope_start: 20,
        scope_end: 80,
        context_id,
        type_name: Some(type_name.into()),
        immutable: true,
        evidence,
        ..LocalBinding::default()
    };
    FileFacts {
        context_order: Vec::new(),
        contexts: vec![
            SemanticStore {
                definitions: vec![definition(0, "excluded")],
                ..SemanticStore::default()
            },
            SemanticStore {
                semantic: semantic(1),
                definitions: vec![definition(1, "included")],
                locals: vec![binding(1, "First", BindingEvidence::Both)],
                ..SemanticStore::default()
            },
            SemanticStore {
                semantic: semantic(2),
                definitions: vec![definition(2, "included")],
                locals: vec![binding(2, "Second", BindingEvidence::Semantic)],
                ..SemanticStore::default()
            },
        ],
        source: Arc::from(vec![b' '; 100]),
        target: PathBuf::from("fixture.rs"),
        aliases: vec![PathBuf::from("fixture.rs")],
        path: PathBuf::from("fixture.rs"),
        module: ModulePath::default(),
        resolution_family: ResolutionFamily::RustCrates,
        file_module_rules: None,
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
    .index_contexts()
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
        &facts[0].contexts[1].definitions[0]
    ));
    assert!(std::ptr::eq(
        projected.file(0).definitions().next().unwrap(),
        &facts[0].contexts[1].definitions[0]
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
        .symbol(ProgramSymbolId {
            file_index: 0,
            definition_index: 0,
        })
        .map(|symbol| &symbol.semantic_id)
        .unwrap();
    assert_eq!(
        projected.unique_symbol_for_target(excluded_target),
        Some(id)
    );
    let ambiguous = project(HashSet::from([1, 2]));
    assert_eq!(ambiguous.unique_symbol_for_target(excluded_target), None);
    assert_eq!(
        ambiguous.unique_symbol_for_target(&graph.symbol(id).unwrap().semantic_id),
        None
    );
}

#[test]
fn projections_preserve_conflicting_binding_evidence_within_an_eligible_context() {
    let mut facts = [fixture()];
    let mut conflicting = facts[0].contexts[2].locals[0].clone();
    conflicting.context_id = 1;
    facts[0].contexts[1].locals.push(conflicting);
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

#[test]
fn context_fragments_keep_record_order_and_scope_tie_selection() {
    let mut fact = fixture();
    let mut first = fact.contexts.remove(1);
    let mut binding = first.locals.pop().unwrap();
    binding.evidence = BindingEvidence::Semantic;
    first.locals.clear();
    first.definitions[0].name = "first".into();
    first.semantic.resolution_family = Some(ResolutionFamily::KotlinModules);
    first.semantic.receivers.push(ReceiverScope {
        start_byte: 20,
        end_byte: 80,
        receiver_type: "Owner".into(),
        label: None,
        implicit: true,
        dsl_markers: Vec::new(),
    });
    let mut middle = first.clone();
    middle.definitions[0].name = "middle".into();
    middle.semantic.resolution_family = Some(ResolutionFamily::PhpNamespaces);
    middle.locals.push(binding.clone());
    let mut last = first.clone();
    last.definitions[0].name = "last".into();
    last.locals.push(LocalBinding {
        scope_start: 30,
        scope_end: 90,
        ..binding
    });
    fact.contexts = vec![first, middle, last];
    let facts = [fact.index_contexts()];
    let graph = ProgramGraph::new(&facts);
    assert_eq!(
        graph
            .file(0)
            .definitions()
            .map(|item| item.name.as_str())
            .collect::<Vec<_>>(),
        ["first", "middle", "last"]
    );
    // The binding and receiver scopes have equal widths. The original merge
    // indexed all Kotlin bindings before receivers and the middle PHP segment.
    assert_eq!(graph.scope_at(0, 1, 50).unwrap().start_byte, 30);
}
