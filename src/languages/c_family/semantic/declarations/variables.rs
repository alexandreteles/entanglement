use super::*;

impl Records<'_> {
    pub(super) fn variable(
        &mut self,
        node: Node<'_>,
        range: Node<'_>,
        declarator: Node<'_>,
        name: Node<'_>,
    ) {
        let source = self.source;
        let kind = node.kind();
        let owner = if kind == "for_range_loop" {
            node
        } else {
            scope(node)
        };
        if kind.contains("parameter_declaration")
            && !matches!(
                owner.kind(),
                "function_definition" | "lambda_expression" | "catch_clause"
            )
        {
            return;
        }
        let type_name = if name
            .parent()
            .is_some_and(|parent| parent.kind() == "structured_binding_declarator")
        {
            None
        } else {
            variable_type(node, declarator, source)
        };
        let scope_start = if kind == "for_range_loop" {
            node.child_by_field_name("body")
                .map_or(node.end_byte(), |body| body.start_byte())
        } else {
            name.end_byte()
        };
        let name = text(name, source);
        if matches!(owner.kind(), "translation_unit" | "field_declaration_list")
            && !kind.contains("parameter_declaration")
        {
            let storage = |word: &str| {
                named_children(node).any(|child| {
                    child.kind() == "storage_class_specifier" && text(child, source) == word
                })
            };
            let member = kind == "field_declaration";
            let symbol = Symbol {
                result: type_name.clone(),
                body: !storage("extern") || declarator.kind() == "init_declarator",
                internal: !member && storage("static"),
                member,
                instance_field: member && !storage("static"),
                ..Default::default()
            };
            let kind = if member {
                DefinitionKind::Other
            } else {
                DefinitionKind::Static
            };
            self.add(range, name, declarator::owners(node, source), kind, symbol);
            return;
        }
        self.store.locals.push(LocalBinding {
            name,
            start_byte: declarator.start_byte(),
            end_byte: declarator.end_byte(),
            scope_start,
            scope_end: owner.end_byte(),
            type_name,
            callable_reference: declarator.kind() == "function_declarator",
            ..Default::default()
        });
    }
}

fn variable_type(node: Node<'_>, mut declaration: Node<'_>, source: &[u8]) -> Option<String> {
    let ty = node.child_by_field_name("type")?;
    let mut words: Vec<_> = named_children(node)
        .filter(|child| child.kind() == "type_qualifier")
        .map(|child| text(child, source))
        .collect();
    words.push(text(ty, source));
    let mut written = words.join(" ");
    while let Some(child) = declaration.child_by_field_name("declarator") {
        match declaration.kind() {
            "pointer_declarator" => written.push('*'),
            "reference_declarator" => written.push('&'),
            "array_declarator" => written.push('*'),
            _ => {}
        }
        declaration = child;
    }
    Some(written)
}
