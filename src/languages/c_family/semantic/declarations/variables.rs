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
        let owner = scope(node);
        if kind.contains("parameter_declaration")
            && !matches!(owner.kind(), "function_definition" | "lambda_expression")
        {
            return;
        }
        let name = text(name, source);
        let type_name = variable_type(node, declarator, source);
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
            scope_start: owner.start_byte(),
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
