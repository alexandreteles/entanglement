use super::*;

mod callables;
mod types;
mod variables;

struct Records<'a> {
    source: &'a [u8],
    data: &'a mut Data,
    store: &'a mut SemanticStore,
}

pub(super) fn capture(node: Node<'_>, source: &[u8], data: &mut Data, store: &mut SemanticStore) {
    let mut records = Records {
        source,
        data,
        store,
    };
    match node.kind() {
        "class_specifier" | "struct_specifier" | "enum_specifier" | "union_specifier" => {
            records.aggregate(node)
        }
        "lambda_expression" => records.lambda(node),
        "enumerator" => records.enumerator(node),
        "alias_declaration" => {
            if let Some(name) = node.child_by_field_name("name") {
                records.alias(node, name);
            }
        }
        _ => {
            let mut cursor = node.walk();
            for declaration in node.children_by_field_name("declarator", &mut cursor) {
                let range = if node.kind() == "function_definition" || node.named_child_count() <= 2
                {
                    node
                } else {
                    declaration
                };
                if node.kind() == "type_definition" {
                    if let Some(name) = declarator::identifier(declaration) {
                        records.alias(range, name);
                    }
                } else if !node.kind().contains("parameter_declaration")
                    && let Some(callable) = declarator::callable(declaration, source)
                {
                    records.function(node, range, declaration, callable);
                } else if let Some(name) = declarator::identifier(declaration) {
                    if name.kind() == "structured_binding_declarator" {
                        for binding in
                            named_children(name).filter(|child| child.kind() == "identifier")
                        {
                            records.variable(node, binding, declaration, binding);
                        }
                    } else {
                        records.variable(node, range, declaration, name);
                    }
                }
            }
        }
    }
}

impl Records<'_> {
    fn add(
        &mut self,
        node: Node<'_>,
        name: String,
        owners: Vec<String>,
        kind: DefinitionKind,
        mut symbol: Symbol,
    ) {
        let local = scope(node);
        symbol.exported |= exported(node);
        self.store.definitions.push(Definition {
            name: name.clone(),
            kind,
            module: ModulePath(owners),
            start_byte: node.start_byte(),
            end_byte: node.end_byte(),
            scope_start: local.start_byte(),
            scope_end: local.end_byte(),
            is_public: !symbol.internal && visibility(node, self.source) != "private",
            inline_module: false,
            external_module: false,
            context_id: 0,
            declaration: None,
        });
        self.data.symbols.insert((node.start_byte(), name), symbol);
    }
}
