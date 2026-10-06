use crate::model::{DefinitionKind, ReferenceKind};

use super::super::super::query::QueryAnalyzer;

pub(crate) struct Captures {
    pub(crate) import: Option<u32>,
    pub(crate) export: Option<u32>,
    pub(crate) name: Option<u32>,
    pub(crate) definitions: Vec<(u32, DefinitionKind)>,
    pub(crate) references: Vec<(u32, ReferenceKind)>,
    pub(crate) namespace: Option<u32>,
    pub(crate) local_binding: Option<u32>,
    pub(crate) local_parameter: Option<u32>,
    pub(crate) local_catch: Option<u32>,
    pub(crate) local_loop: Option<u32>,
}

impl Captures {
    pub(crate) fn new(query: &QueryAnalyzer) -> Self {
        let capture = |name| query.capture_id(name);
        Self {
            import: capture("import"),
            export: capture("export"),
            name: capture("definition.name"),
            definitions: [
                ("definition.function", DefinitionKind::Function),
                ("definition.method", DefinitionKind::Method),
                ("definition.class", DefinitionKind::Other),
                ("definition.interface", DefinitionKind::Trait),
                ("definition.type", DefinitionKind::TypeAlias),
                ("definition.enum", DefinitionKind::Enum),
                ("definition.variable", DefinitionKind::Other),
            ]
            .into_iter()
            .filter_map(|(name, kind)| capture(name).map(|id| (id, kind)))
            .collect(),
            references: [
                ("reference.call", ReferenceKind::Call),
                ("reference.method", ReferenceKind::Method),
                ("reference.type", ReferenceKind::Type),
                ("reference.value", ReferenceKind::Value),
            ]
            .into_iter()
            .filter_map(|(name, kind)| capture(name).map(|id| (id, kind)))
            .collect(),
            namespace: capture("reference.namespace"),
            local_binding: capture("local.binding"),
            local_parameter: capture("local.parameter"),
            local_catch: capture("local.catch"),
            local_loop: capture("local.loop"),
        }
    }
}
