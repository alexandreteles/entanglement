use tree_sitter::Node;

use crate::model::{Export, Import, ModulePath};

pub(super) fn import_fact(
    statement: Node<'_>,
    module_source: &str,
    imported_name: Option<String>,
    alias: Option<String>,
    namespace: bool,
) -> Import {
    Import {
        path: Vec::new(),
        alias,
        source: Some(module_source.to_owned()),
        imported_name,
        namespace,
        start_byte: statement.start_byte(),
        end_byte: statement.end_byte(),
        module: ModulePath::default(),
        scope_start: 0,
        scope_end: 0,
        is_public: false,
        context_id: 0,
    }
}

pub(super) fn export_fact(
    statement: Node<'_>,
    module_source: Option<String>,
    imported_name: Option<String>,
    local_name: Option<String>,
    exported_name: String,
    namespace: bool,
) -> Export {
    Export {
        source: module_source,
        imported_name,
        local_name,
        exported_name,
        namespace,
        start_byte: statement.start_byte(),
        end_byte: statement.end_byte(),
        scope_start: 0,
        scope_end: 0,
        context_id: 0,
    }
}
