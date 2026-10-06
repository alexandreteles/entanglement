use tree_sitter::Node;

use crate::model::{Definition, DefinitionKind, ModulePath};

use super::CaptureFacts;

impl CaptureFacts {
    pub(super) fn add_default_callable_definition(&mut self, statement: Node<'_>) {
        let Some(callable) = super::super::exports::default_callable(statement) else {
            return;
        };
        let range = callable.byte_range();
        self.semantic.definitions.push(Definition {
            name: "default".to_owned(),
            kind: DefinitionKind::Function,
            module: ModulePath::default(),
            start_byte: range.start,
            end_byte: range.end,
            scope_start: self.root.start,
            scope_end: self.root.end,
            is_public: true,
            inline_module: false,
            external_module: false,
            context_id: 0,
        });
    }
}
